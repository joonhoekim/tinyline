//! xdg-shell: 표면에 "창"이라는 역할을 준다.
//!
//! 표면만으로는 화면 어디에, 얼마만 한 크기로 놓일지 알 수 없다. xdg_toplevel 역할을 받은 표면은
//! 컴포지터가 정해 주는 크기(configure)를 받고, 받았다고 답(ack_configure)한 뒤에 그림을 붙인다.

use crate::protocol::{Client, Object, ProtocolError, Shared};
use crate::wire::{Args, MessageBuilder};

/// 표면 하나에 붙은 xdg_surface.
#[derive(Debug)]
pub struct XdgSurface {
    pub surface: u32,
    pub toplevel: Option<u32>,
    /// 컴포지터가 보낸 configure를 클라이언트가 한 번이라도 확인했다.
    pub acked: bool,
}

/// 창 하나(xdg_toplevel).
#[derive(Debug)]
pub struct Toplevel {
    pub xdg_surface: u32,
    pub title: String,
    /// 마지막으로 알려 준 (너비, 높이, 포커스 여부). 같으면 다시 보내지 않는다.
    pub configured: Option<(u32, u32, bool)>,
}

/// xdg_toplevel.configure의 states 배열에 넣는 값: "이 창이 지금 포커스를 받았다".
const STATE_ACTIVATED: u32 = 4;

impl Client {
    pub(crate) fn handle_xdg(
        &mut self,
        object: Object,
        id: u32,
        opcode: u16,
        mut args: Args<'_>,
        shared: &mut Shared,
    ) -> Result<(), ProtocolError> {
        match (object, opcode) {
            // xdg_wm_base.destroy
            (Object::WmBase, 0) => self.destroy(id),
            // xdg_wm_base.create_positioner(id): 팝업 위치용. 받아 두기만 한다.
            (Object::WmBase, 1) => {
                let positioner = args.uint()?;
                self.create(positioner, Object::Positioner)?;
            }
            // xdg_wm_base.get_xdg_surface(id, surface)
            (Object::WmBase, 2) => {
                let xdg_id = args.uint()?;
                let surface_id = args.uint()?;
                let surface = self
                    .surfaces
                    .get_mut(&surface_id)
                    .ok_or(ProtocolError::UnknownObject(surface_id))?;
                if surface.xdg_surface.is_some() || surface.image.is_some() {
                    return Err(ProtocolError::Object {
                        object: id,
                        code: 0, // xdg_wm_base.error.role
                        message: format!("surface {surface_id} already has content or a role"),
                    });
                }
                surface.xdg_surface = Some(xdg_id);
                self.create(xdg_id, Object::XdgSurface)?;
                self.xdg_surfaces.insert(
                    xdg_id,
                    XdgSurface {
                        surface: surface_id,
                        toplevel: None,
                        acked: false,
                    },
                );
            }
            // xdg_wm_base.pong: 이 컴포지터는 ping을 보내지 않는다.
            (Object::WmBase, 3) => {}

            (Object::Positioner, 0) => self.destroy(id),
            (Object::Positioner, _) => {}

            // xdg_surface.destroy
            (Object::XdgSurface, 0) => {
                if let Some(xdg) = self.xdg_surfaces.remove(&id)
                    && let Some(surface) = self.surfaces.get_mut(&xdg.surface)
                {
                    surface.xdg_surface = None;
                }
                self.destroy(id);
            }
            // xdg_surface.get_toplevel(id): 이 표면이 창이 된다. 데스크톱에 자리를 잡는다.
            (Object::XdgSurface, 1) => {
                let toplevel_id = args.uint()?;
                self.create(toplevel_id, Object::Toplevel)?;
                let xdg = self.xdg_surfaces.get_mut(&id).expect("xdg_surface exists");
                xdg.toplevel = Some(toplevel_id);
                let surface = xdg.surface;
                self.toplevels.insert(
                    toplevel_id,
                    Toplevel {
                        xdg_surface: id,
                        title: String::new(),
                        configured: None,
                    },
                );
                shared.show(self.id, surface);
                shared.dirty = true;
            }
            // xdg_surface.get_popup: 이 컴포지터는 팝업을 지원하지 않는다.
            (Object::XdgSurface, 2) => {
                return Err(ProtocolError::InvalidMethod { object: id, opcode });
            }
            // xdg_surface.set_window_geometry: 그림자 등을 뺀 실제 창 영역. 무시한다.
            (Object::XdgSurface, 3) => {}
            // xdg_surface.ack_configure(serial)
            (Object::XdgSurface, 4) => {
                let _serial = args.uint()?;
                self.xdg_surfaces.get_mut(&id).expect("exists").acked = true;
            }

            // xdg_toplevel.destroy: 창이 사라진다.
            (Object::Toplevel, 0) => {
                if let Some(toplevel) = self.toplevels.remove(&id) {
                    let xdg = self.xdg_surfaces.get_mut(&toplevel.xdg_surface);
                    if let Some(xdg) = xdg {
                        xdg.toplevel = None;
                        shared.hide(self.id, xdg.surface);
                    }
                }
                self.destroy(id);
            }
            // xdg_toplevel.set_title(title)
            (Object::Toplevel, 2) => {
                let title = args.string()?.to_string();
                let toplevel = self.toplevels.get_mut(&id).expect("exists");
                let surface = self.xdg_surfaces[&toplevel.xdg_surface].surface;
                if let Some(window) = shared.window_for(self.id, surface) {
                    shared.desktop.set_title(window, &title);
                }
                toplevel.title = title;
            }
            // 나머지(부모 창, 최대화, 이동·크기 조절 요청 …)는 이 컴포지터가 무시한다.
            (Object::Toplevel, _) => {}
            _ => return Err(ProtocolError::InvalidMethod { object: id, opcode }),
        }
        Ok(())
    }

    /// 창마다 데스크톱이 정한 크기와 포커스를 알려 준다. 바뀐 창에만 보낸다.
    ///
    /// 첫 configure는 클라이언트가 표면을 한 번 commit한 뒤에 보내야 한다(xdg-shell의 규칙).
    pub fn configure_toplevels(&mut self, shared: &mut Shared) {
        let toplevel_ids: Vec<u32> = self.toplevels.keys().copied().collect();
        for toplevel_id in toplevel_ids {
            let xdg_id = self.toplevels[&toplevel_id].xdg_surface;
            let surface_id = self.xdg_surfaces[&xdg_id].surface;
            if self.surfaces[&surface_id].commits == 0 {
                continue;
            }
            let Some(window_id) = shared.window_for(self.id, surface_id) else {
                continue;
            };
            let Some(window) = shared.desktop.windows().iter().find(|w| w.id == window_id) else {
                continue;
            };
            let activated = shared.desktop.focused() == Some(window_id);
            let wanted = (window.rect.width, window.rect.height, activated);
            if self.toplevels[&toplevel_id].configured == Some(wanted) {
                continue;
            }
            self.toplevels
                .get_mut(&toplevel_id)
                .expect("exists")
                .configured = Some(wanted);

            let (width, height, activated) = wanted;
            let mut states = Vec::new();
            if activated {
                states.extend_from_slice(&STATE_ACTIVATED.to_ne_bytes());
            }
            let serial = shared.next_serial();
            self.send(
                MessageBuilder::new(toplevel_id, 0)
                    .int(i32::try_from(width).unwrap_or(i32::MAX))
                    .int(i32::try_from(height).unwrap_or(i32::MAX))
                    .array(&states)
                    .finish(),
            );
            self.send(MessageBuilder::new(xdg_id, 0).uint(serial).finish());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wl_compositor(4)·xdg_wm_base(5)를 bind하고 표면 6 → xdg_surface 7 → toplevel 8을 만든 클라이언트.
    fn toplevel_client(shared: &mut Shared) -> Client {
        let mut client = Client::new(1);
        let mut bytes = MessageBuilder::new(1, 1).uint(2).finish();
        for (name, interface, id) in [(1, "wl_compositor", 4), (3, "xdg_wm_base", 5)] {
            bytes.extend(
                MessageBuilder::new(2, 0)
                    .uint(name)
                    .string(interface)
                    .uint(1)
                    .uint(id)
                    .finish(),
            );
        }
        bytes.extend(MessageBuilder::new(4, 0).uint(6).finish());
        bytes.extend(MessageBuilder::new(5, 2).uint(7).uint(6).finish());
        bytes.extend(MessageBuilder::new(7, 1).uint(8).finish());
        client.receive(&bytes, shared).unwrap();
        client.outgoing.clear();
        client
    }

    #[test]
    fn toplevel_gets_a_window_and_configure_after_first_commit() {
        let mut shared = Shared::default();
        let mut client = toplevel_client(&mut shared);
        assert_eq!(shared.desktop.windows().len(), 1);

        client.configure_toplevels(&mut shared);
        assert!(
            client.outgoing.is_empty(),
            "no configure before the first commit"
        );

        client
            .receive(&MessageBuilder::new(6, 6).finish(), &mut shared)
            .unwrap();
        client.configure_toplevels(&mut shared);
        assert!(!client.outgoing.is_empty());
        let (width, height, activated) = client.toplevels[&8].configured.unwrap();
        assert_eq!((width, height), (800, 480));
        assert!(activated);

        client.outgoing.clear();
        client.configure_toplevels(&mut shared);
        assert!(client.outgoing.is_empty(), "nothing changed, nothing sent");
    }

    #[test]
    fn buffer_before_ack_is_an_error() {
        let mut shared = Shared::default();
        let mut client = toplevel_client(&mut shared);
        // 버퍼 9가 있다고 치고 attach + commit
        let mut bytes = MessageBuilder::new(6, 1).uint(9).int(0).int(0).finish();
        bytes.extend(MessageBuilder::new(6, 6).finish());
        assert!(matches!(
            client.receive(&bytes, &mut shared),
            Err(ProtocolError::Object {
                object: 7,
                code: 3,
                ..
            })
        ));
    }

    #[test]
    fn destroying_the_toplevel_closes_the_window() {
        let mut shared = Shared::default();
        let mut client = toplevel_client(&mut shared);
        client
            .receive(&MessageBuilder::new(8, 0).finish(), &mut shared)
            .unwrap();
        assert!(shared.desktop.windows().is_empty());
    }
}

//! Bridges the `xwayland-keyboard-grab-unstable-v1` protocol (which the real Xwayland
//! process speaks to satellite when an X11 client performs an active keyboard grab,
//! e.g. via `XGrabKeyboard`, as VMware/VirtualBox/remote-desktop clients do) to the
//! `keyboard-shortcuts-inhibit-unstable-v1` protocol on the host compositor side.
//!
//! Without this, host compositors that implement keyboard-shortcuts-inhibit (such as
//! niri) have no way to learn that an Xwayland client currently holds an active X11
//! keyboard grab, and will keep processing their own global keybinds instead of
//! forwarding all keys to the grabbing client's window.

use super::*;
use log::warn;
use wayland_protocols::xwayland::keyboard_grab::zv1::server::{
    zwp_xwayland_keyboard_grab_manager_v1::{self, ZwpXwaylandKeyboardGrabManagerV1},
    zwp_xwayland_keyboard_grab_v1::{self, ZwpXwaylandKeyboardGrabV1},
};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1;
use wayland_server::{DataInit, Dispatch, GlobalDispatch, New};

/// Per-grab state attached to the server-side `zwp_xwayland_keyboard_grab_v1` resource.
/// Holds the corresponding client-side inhibitor object (if the host compositor
/// supports the shortcuts-inhibit protocol and the surface/seat could be resolved),
/// so it can be torn down when Xwayland destroys the grab.
#[derive(Default)]
pub(super) struct XwaylandKeyboardGrab {
    inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
}

impl<S: X11Selection> GlobalDispatch<ZwpXwaylandKeyboardGrabManagerV1, ()>
    for InnerServerState<S>
{
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwpXwaylandKeyboardGrabManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl<S: X11Selection> Dispatch<ZwpXwaylandKeyboardGrabManagerV1, ()> for InnerServerState<S> {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwpXwaylandKeyboardGrabManagerV1,
        request: <ZwpXwaylandKeyboardGrabManagerV1 as Resource>::Request,
        _: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwp_xwayland_keyboard_grab_manager_v1::Request;
        match request {
            Request::GrabKeyboard { id, surface, seat } => {
                let mut grab = XwaylandKeyboardGrab::default();

                if let Some(manager) = state.keyboard_shortcuts_inhibit_manager.as_ref() {
                    let surface_entity = surface.data::<Entity>().copied();
                    let seat_entity = seat.data::<Entity>().copied();

                    let host_surface = surface_entity
                        .and_then(|e| state.world.get::<&client::wl_surface::WlSurface>(e).ok());
                    let host_seat = seat_entity
                        .and_then(|e| state.world.get::<&client::wl_seat::WlSeat>(e).ok());

                    match (host_surface, host_seat) {
                        (Some(host_surface), Some(host_seat)) => {
                            let inhibitor = manager.inhibit_shortcuts(
                                &host_surface,
                                &host_seat,
                                &state.qh,
                                (),
                            );
                            grab.inhibitor = Some(inhibitor);
                        }
                        _ => {
                            warn!(
                                "couldn't resolve host surface/seat for xwayland keyboard grab"
                            );
                        }
                    }
                }

                data_init.init(id, grab);
            }
            Request::Destroy => {}
            _ => {}
        }
    }
}

impl<S: X11Selection> Dispatch<ZwpXwaylandKeyboardGrabV1, XwaylandKeyboardGrab>
    for InnerServerState<S>
{
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZwpXwaylandKeyboardGrabV1,
        request: <ZwpXwaylandKeyboardGrabV1 as Resource>::Request,
        data: &XwaylandKeyboardGrab,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwp_xwayland_keyboard_grab_v1::Request::Destroy = request {
            if let Some(inhibitor) = data.inhibitor.as_ref() {
                inhibitor.destroy();
            }
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &ZwpXwaylandKeyboardGrabV1,
        data: &XwaylandKeyboardGrab,
    ) {
        if let Some(inhibitor) = data.inhibitor.as_ref() {
            inhibitor.destroy();
        }
    }
}

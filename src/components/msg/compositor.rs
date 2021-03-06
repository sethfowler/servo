/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use azure::azure_hl::DrawTarget;
use azure::azure::AzGLContext;
use geom::rect::Rect;
use geom::size::Size2D;

pub struct LayerBuffer {
    draw_target: DrawTarget,

    // The rect in the containing RenderLayer that this represents.
    rect: Rect<f32>,

    // The rect in pixels that will be drawn to the screen.
    screen_pos: Rect<uint>,

    // NB: stride is in pixels, like OpenGL GL_UNPACK_ROW_LENGTH.
    stride: uint
}

/// A set of layer buffers. This is an atomic unit used to switch between the front and back
/// buffers.
pub struct LayerBufferSet {
    buffers: ~[LayerBuffer]
}

/// The status of the renderer.
#[deriving(Eq)]
pub enum RenderState {
    IdleRenderState,
    RenderingRenderState,
}

/// The interface used by the renderer to acquire draw targets for each rendered frame and
/// submit them to be drawn to the display.
pub trait RenderListener {
    fn get_gl_context(&self) -> AzGLContext;
    fn paint(&self, layer_buffer_set: LayerBufferSet, new_size: Size2D<uint>);
    fn set_render_state(&self, render_state: RenderState);
}

pub enum ReadyState {
    /// Informs the compositor that a page is loading. Used for setting status
    Loading,
    /// Informs the compositor that a page is performing layout. Used for setting status
    PerformingLayout,
    /// Informs the compositor that a page is finished loading. Used for setting status
    FinishedLoading,
}

/// The interface used by the script task to tell the compositor to update its ready state,
/// which is used in displaying the appropriate message in the window's title.
pub trait ScriptListener : Clone {
    fn set_ready_state(&self, ReadyState);
}

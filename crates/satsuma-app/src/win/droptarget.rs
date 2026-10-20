//! OLE drop targets: the wheel and the Float shelf accept file drags from any app
//! (`CF_HDROP`), the Windows counterpart of `NSDraggingDestination`.

use std::path::PathBuf;
use std::rc::Rc;
use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, DROPEFFECT, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// What a window does with a drag. Points are screen pixels.
pub trait DropHandler {
    /// `files` is `None` when the drag carries no file paths.
    fn enter(&self, files: Option<Vec<PathBuf>>, x: i32, y: i32) -> DROPEFFECT;
    fn over(&self, x: i32, y: i32) -> DROPEFFECT;
    fn leave(&self);
    fn dropped(&self, files: Option<Vec<PathBuf>>, x: i32, y: i32) -> DROPEFFECT;
}

/// File paths in a data object, if it has any (virtual items such as phone photos don't).
pub fn file_paths(data: &IDataObject) -> Option<Vec<PathBuf>> {
    let format = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    unsafe {
        let mut medium = data.GetData(&format).ok()?;
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
        ReleaseStgMedium(&mut medium);
        (!paths.is_empty()).then_some(paths)
    }
}

#[implement(IDropTarget)]
struct Target {
    handler: Rc<dyn DropHandler>,
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let files = data.as_ref().and_then(file_paths);
        let e = self.handler.enter(files, pt.x, pt.y);
        unsafe { *effect = e };
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let e = self.handler.over(pt.x, pt.y);
        unsafe { *effect = e };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.handler.leave();
        Ok(())
    }

    fn Drop(&self, data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let files = data.as_ref().and_then(file_paths);
        let e = self.handler.dropped(files, pt.x, pt.y);
        unsafe { *effect = e };
        Ok(())
    }
}

/// Registers `handler` for drops on `hwnd` until [`revoke`].
pub fn register(hwnd: HWND, handler: Rc<dyn DropHandler>) -> Result<()> {
    let target: IDropTarget = Target { handler }.into();
    unsafe { RegisterDragDrop(hwnd, &target) }
}

pub fn revoke(hwnd: HWND) {
    unsafe {
        let _ = RevokeDragDrop(hwnd);
    }
}

pub const NONE: DROPEFFECT = DROPEFFECT_NONE;

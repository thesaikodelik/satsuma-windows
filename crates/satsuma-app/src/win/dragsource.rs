//! Dragging files out of Satsuma (the Float shelf) with OLE `DoDragDrop`, using the shell's
//! own data object so every app sees the same formats as a drag from File Explorer.

use std::path::{Path, PathBuf};
use windows::core::{implement, Result, HRESULT, HSTRING};
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{ILFree, SHCreateShellItemArrayFromIDLists, SHParseDisplayName, BHID_DataObject};

#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: windows::core::BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// A shell data object for files (CF_HDROP, shell IDs, file names …).
pub fn data_object(paths: &[PathBuf]) -> Result<IDataObject> {
    unsafe {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
        for p in paths {
            let mut pidl = std::ptr::null_mut();
            SHParseDisplayName(&HSTRING::from(p.as_os_str()), None, &mut pidl, 0, None)?;
            pidls.push(pidl);
        }
        // A shell item array renders the same formats as File Explorer (CF_HDROP included),
        // even for items in different folders.
        let ids: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
        let result = SHCreateShellItemArrayFromIDLists(&ids).and_then(|items| items.BindToHandler::<_, IDataObject>(None, &BHID_DataObject));
        for p in pidls {
            ILFree(Some(p));
        }
        result
    }
}

/// Runs a drag of `paths` until it is dropped or cancelled (the mouse button must be down).
pub fn drag_files(paths: &[PathBuf]) -> Result<DROPEFFECT> {
    let data = data_object(paths)?;
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT::default();
    let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect) };
    if hr.is_err() {
        return Err(hr.into());
    }
    Ok(effect)
}

#[allow(dead_code)]
pub fn exists(p: &Path) -> bool {
    p.exists()
}

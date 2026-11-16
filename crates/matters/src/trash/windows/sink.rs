//! The progress sink of the Recycle Bin's `IFileOperation` (`super`): it refuses a delete
//! that would not recycle and records how the delete went. `windows-core`'s `implement` builds
//! its COM object; the expect below covers that macro's own code.
#![expect(
    clippy::ref_as_ptr,
    clippy::inline_always,
    reason = "the code windows-core's implement macro generates for the sink"
)]

use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::Foundation::E_ABORT;
use windows::Win32::UI::Shell::{
    IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, PCWSTR, Ref};
use windows_core::implement;

/// What the progress sink saw of the one delete.
#[derive(Default)]
pub(super) struct Seen {
    /// `PreDeleteItem` refused: the shell would have deleted the file for good.
    pub(super) refused: bool,
    /// `PostDeleteItem`'s result.
    pub(super) result: Option<HRESULT>,
    /// Where the Recycle Bin keeps the file, from `PostDeleteItem`'s new item.
    pub(super) recycled: Option<PathBuf>,
}

/// The progress sink: refuses a delete that would not recycle, and records the outcome. Its
/// state is behind a mutex, since the shell may call it from another thread when the caller is
/// in the multithreaded apartment.
#[implement(IFileOperationProgressSink)]
#[derive(Default)]
pub(super) struct Sink {
    seen: Mutex<Seen>,
}

impl Sink {
    pub(super) fn with_seen<T>(&self, f: impl FnOnce(&mut Seen) -> T) -> T {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut seen)
    }
}

impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn PreDeleteItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let _ = psiitem;
        if dwflags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0.cast_unsigned() == 0 {
            self.with_seen(|seen| seen.refused = true);
            return Err(E_ABORT.into());
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        hrdelete: HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let recycled = psinewlycreated.as_ref().and_then(super::file_path);
        self.with_seen(|seen| {
            seen.result = Some(hrdelete);
            seen.recycled = recycled;
        });
        Ok(())
    }

    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _hrresult: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hrrename: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostMoveItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hrmove: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreCopyItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostCopyItem(
        &self,
        _dwflags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hrcopy: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreNewItem(
        &self,
        _dwflags: u32,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _dwflags: u32,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _template_name: &PCWSTR,
        _attributes: u32,
        _hrnew: HRESULT,
        _new_item: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _total: u32, _so_far: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

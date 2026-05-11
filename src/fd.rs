use crate::file::{AccessMode, FileError, OpenFileId, OpenFileKind, OpenFileTable};

pub const STDIN_FILENO: usize = 0;
pub const STDOUT_FILENO: usize = 1;
pub const STDERR_FILENO: usize = 2;
pub const MAX_FDS_PER_PROCESS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileDescriptor(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdEntry {
    pub open_file: OpenFileId,
}

pub struct FdTable {
    entries: [Option<FdEntry>; MAX_FDS_PER_PROCESS],
}

impl FdTable {
    pub fn new_with_stdio(open_files: &mut OpenFileTable) -> Result<Self, FileError> {
        let mut table = Self::empty();

        let stdin = open_files.alloc(OpenFileKind::NullInput, AccessMode::ReadOnly)?;
        table.entries[STDIN_FILENO] = Some(FdEntry { open_file: stdin });

        let stdout = match open_files.alloc(OpenFileKind::ConsoleStdout, AccessMode::WriteOnly) {
            Ok(id) => id,
            Err(error) => {
                table.close_all(open_files);
                return Err(error);
            }
        };
        table.entries[STDOUT_FILENO] = Some(FdEntry { open_file: stdout });

        let stderr = match open_files.alloc(OpenFileKind::ConsoleStderr, AccessMode::WriteOnly) {
            Ok(id) => id,
            Err(error) => {
                table.close_all(open_files);
                return Err(error);
            }
        };
        table.entries[STDERR_FILENO] = Some(FdEntry { open_file: stderr });

        Ok(table)
    }

    pub const fn empty() -> Self {
        Self {
            entries: [None; MAX_FDS_PER_PROCESS],
        }
    }

    pub fn allocate_lowest(&mut self, open_file: OpenFileId) -> Result<FileDescriptor, FileError> {
        let Some(index) = self.entries.iter().position(Option::is_none) else {
            return Err(FileError::TooManyProcessFiles);
        };

        self.entries[index] = Some(FdEntry { open_file });
        Ok(FileDescriptor(index))
    }

    pub fn get(&self, fd: usize) -> Option<FdEntry> {
        self.entries.get(fd).copied().flatten()
    }

    pub fn is_open(&self, fd: usize) -> bool {
        self.get(fd).is_some()
    }

    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_some()).count()
    }

    pub fn close(&mut self, fd: usize, open_files: &mut OpenFileTable) -> Result<(), FileError> {
        let Some(entry_slot) = self.entries.get_mut(fd) else {
            return Err(FileError::BadFd);
        };
        let Some(entry) = entry_slot.take() else {
            return Err(FileError::BadFd);
        };

        open_files.dec_ref(entry.open_file);
        Ok(())
    }

    pub fn close_all(&mut self, open_files: &mut OpenFileTable) {
        for entry in &mut self.entries {
            if let Some(entry) = entry.take() {
                open_files.dec_ref(entry.open_file);
            }
        }
    }
}

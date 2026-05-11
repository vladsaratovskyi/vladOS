use alloc::vec::Vec;

pub const MAX_OPEN_FILES: usize = 32;
pub const MAX_PATH_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFileId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedFileId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    ReadOnly,
    WriteOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFileKind {
    NullInput,
    ConsoleStdout,
    ConsoleStderr,
    EmbeddedFile(EmbeddedFileId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileError {
    BadFd,
    NoEntry,
    TooManySystemFiles,
    TooManyProcessFiles,
    NameTooLong,
    Invalid,
    Fault,
}

pub struct OpenFile {
    kind: OpenFileKind,
    access: AccessMode,
    offset: usize,
    ref_count: usize,
}

impl OpenFile {
    pub fn kind(&self) -> OpenFileKind {
        self.kind
    }

    pub fn access(&self) -> AccessMode {
        self.access
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }
}

pub struct OpenFileTable {
    files: Vec<Option<OpenFile>>,
}

impl OpenFileTable {
    pub const fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn alloc(
        &mut self,
        kind: OpenFileKind,
        access: AccessMode,
    ) -> Result<OpenFileId, FileError> {
        self.ensure_slots();

        if let Some(index) = self.files.iter().position(Option::is_none) {
            self.files[index] = Some(OpenFile {
                kind,
                access,
                offset: 0,
                ref_count: 1,
            });
            return Ok(OpenFileId(index));
        }

        Err(FileError::TooManySystemFiles)
    }

    fn ensure_slots(&mut self) {
        if self.files.is_empty() {
            self.files.resize_with(MAX_OPEN_FILES, || None);
        }
    }

    pub fn get(&self, id: OpenFileId) -> Option<&OpenFile> {
        self.files.get(id.0)?.as_ref()
    }

    pub fn get_mut(&mut self, id: OpenFileId) -> Option<&mut OpenFile> {
        self.files.get_mut(id.0)?.as_mut()
    }

    pub fn inc_ref(&mut self, id: OpenFileId) {
        if let Some(file) = self.get_mut(id) {
            file.ref_count += 1;
        }
    }

    pub fn dec_ref(&mut self, id: OpenFileId) {
        let Some(slot) = self.files.get_mut(id.0) else {
            return;
        };
        let Some(file) = slot.as_mut() else {
            return;
        };

        file.ref_count = file.ref_count.saturating_sub(1);
        if file.ref_count == 0 {
            *slot = None;
        }
    }

    pub fn active_count(&self) -> usize {
        self.files.iter().filter(|file| file.is_some()).count()
    }
}

pub struct EmbeddedFile {
    pub path: &'static [u8],
    pub bytes: &'static [u8],
}

pub const EMBEDDED_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        path: b"/hello.txt",
        bytes: b"hello from embedded file\n",
    },
    EmbeddedFile {
        path: b"/motd",
        bytes: b"tiny kernel says hello\n",
    },
];

pub fn find_embedded_file(path: &[u8]) -> Option<EmbeddedFileId> {
    EMBEDDED_FILES
        .iter()
        .position(|file| file.path == path)
        .map(EmbeddedFileId)
}

pub fn embedded_file(id: EmbeddedFileId) -> Option<&'static EmbeddedFile> {
    EMBEDDED_FILES.get(id.0)
}

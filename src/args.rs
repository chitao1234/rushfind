use std::ffi::{OsStr, OsString};

#[derive(Clone, Copy, Debug)]
pub struct Arg<'a> {
    inner: &'a OsStr,
}

impl<'a> Arg<'a> {
    pub fn new(inner: &'a OsStr) -> Self {
        Self { inner }
    }

    pub fn matches(self, value: &str) -> bool {
        self.inner == OsStr::new(value)
    }

    pub fn as_os_str(self) -> &'a OsStr {
        self.inner
    }

    pub fn starts_with_dash(self) -> bool {
        self.inner.as_encoded_bytes().first() == Some(&b'-')
    }

    pub fn display(self) -> String {
        self.inner.to_string_lossy().into_owned()
    }

    pub fn to_os_string(self) -> OsString {
        self.inner.to_os_string()
    }
}

pub struct ArgCursor<'a> {
    tokens: &'a [OsString],
    index: usize,
}

impl<'a> ArgCursor<'a> {
    pub fn new(tokens: &'a [OsString]) -> Self {
        Self { tokens, index: 0 }
    }

    pub fn peek(&self) -> Option<Arg<'a>> {
        self.tokens
            .get(self.index)
            .map(|value| Arg::new(value.as_os_str()))
    }

    pub fn bump(&mut self) -> Option<Arg<'a>> {
        let value = self.peek();
        if value.is_some() {
            self.index += 1;
        }
        value
    }
}

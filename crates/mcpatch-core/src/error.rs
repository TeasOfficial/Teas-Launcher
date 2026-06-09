use std::fmt;

pub type BusinessResult<T> = Result<T, BusinessError>;

/// 业务错误
#[derive(Debug)]
pub struct BusinessError {
    pub reason: String,
}

impl BusinessError {
    pub fn new(reason: impl AsRef<str>) -> Self {
        Self {
            reason: reason.as_ref().to_owned(),
        }
    }
}

impl<S: AsRef<str>> From<S> for BusinessError {
    fn from(value: S) -> Self {
        BusinessError {
            reason: value.as_ref().to_owned(),
        }
    }
}

impl fmt::Display for BusinessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for BusinessError {}

/// 为 Result<T, E> 添加 `.be()` 方法，将错误转换为 BusinessError
pub trait ResultToBusinessError<T, E> {
    fn be<S: AsRef<str>>(self, map: impl FnOnce(E) -> S) -> BusinessResult<T>;
}

impl<T, E> ResultToBusinessError<T, E> for Result<T, E> {
    fn be<S: AsRef<str>>(self, map: impl FnOnce(E) -> S) -> BusinessResult<T> {
        match self {
            Ok(d) => Ok(d),
            Err(e) => Err(BusinessError::new(map(e))),
        }
    }
}

/// 为 Option<T> 添加 `.be()` 方法
pub trait OptionToBusinessError<T> {
    fn be<S: AsRef<str>>(self, map: impl FnOnce() -> S) -> BusinessResult<T>;
}

impl<T> OptionToBusinessError<T> for Option<T> {
    fn be<S: AsRef<str>>(self, map: impl FnOnce() -> S) -> BusinessResult<T> {
        match self {
            Some(d) => Ok(d),
            None => Err(BusinessError::new(map())),
        }
    }
}

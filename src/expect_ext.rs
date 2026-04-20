use std::fmt::Debug;

#[inline(never)]
/// Runtime function.
pub fn abort_with_message(message: &str) -> ! {
    eprintln!("{message}");
    std::process::abort();
}

/// Runtime trait.
pub trait ExpectValueExt<T> {
    /// Runtime trait method.
    fn unwrap_or_abort(self) -> T;
    /// Runtime trait method.
    fn expect_or_abort(self, message: &str) -> T;
}

impl<T> ExpectValueExt<T> for Option<T> {
    fn unwrap_or_abort(self) -> T {
        self.unwrap_or_else(|| abort_with_message("expected option to contain a value"))
    }

    fn expect_or_abort(self, message: &str) -> T {
        self.unwrap_or_else(|| abort_with_message(message))
    }
}

impl<T, E> ExpectValueExt<T> for Result<T, E>
where
    E: Debug,
{
    fn unwrap_or_abort(self) -> T {
        match self {
            Ok(value) => value,
            Err(error) => abort_with_message(&format!("expected Ok(_), got Err({error:?})")),
        }
    }

    fn expect_or_abort(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => abort_with_message(&format!("{message}: {error:?}")),
        }
    }
}

/// Runtime trait.
pub trait ExpectErrExt<E> {
    /// Runtime trait method.
    fn unwrap_err_or_abort(self) -> E;
    /// Runtime trait method.
    fn expect_err_or_abort(self, message: &str) -> E;
}

impl<T, E> ExpectErrExt<E> for Result<T, E>
where
    E: Debug,
{
    fn unwrap_err_or_abort(self) -> E {
        match self {
            Ok(_) => abort_with_message("expected Err(_), got Ok(_)"),
            Err(error) => error,
        }
    }

    fn expect_err_or_abort(self, message: &str) -> E {
        match self {
            Ok(_) => abort_with_message(message),
            Err(error) => error,
        }
    }
}

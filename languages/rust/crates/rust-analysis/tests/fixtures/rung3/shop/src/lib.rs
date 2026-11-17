//! No `std`: no `Option`, `String` or `Vec`, so every call here is one a load without the standard library can still resolve.

pub struct Order {
    pub total: u32,
}

impl Order {
    pub fn new() -> Order {
        Order { total: 0 }
    }

    pub fn make() -> Self {
        Self::new()
    }

    pub fn validate(&self) -> bool {
        self.total == 0
    }

    pub fn submit(&self) -> bool {
        self.validate()
    }
}

pub fn helper() -> u32 {
    1
}

#[cfg(unix)]
pub fn on_unix() -> u32 {
    helper()
}

#[cfg(windows)]
pub fn on_windows() -> u32 {
    helper()
}

// a module in the package but outside `src/`
#[path = "../extra/more.rs"]
mod more;

pub fn use_more() -> u32 {
    more::more_fn()
}

pub mod sync {
    pub struct Spinlock<T>(std::sync::Mutex<T>);
    impl<T> Spinlock<T> {
        pub const fn new(value: T) -> Self {
            Self(std::sync::Mutex::new(value))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap()
        }
    }
}
#[path = "../../src/ke/time.rs"]
pub mod time;

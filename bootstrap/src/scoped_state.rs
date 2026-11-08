// LLM process defaults stay unchanged outside a hosted execution. Hosted calls
// get a private run meter and provider capability state on their calling thread.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
thread_local! { static STATE: RefCell<Option<BTreeMap<&'static str,u64>>> = const { RefCell::new(None) }; }
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Scope(Option<BTreeMap<&'static str, u64>>);
#[cfg(not(target_arch = "wasm32"))]
impl Scope {
    pub(crate) fn enter() -> Self {
        Self(STATE.with(|s| s.replace(Some(BTreeMap::new()))))
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Scope {
    fn drop(&mut self) {
        STATE.with(|s| s.replace(self.0.take()));
    }
}
macro_rules! scoped {
    ($name:ident, $native:ident, $value:ty, $from:expr) => {
        pub(crate) struct $name {
            key: &'static str,
            default: $value,
            global: std::sync::atomic::$native,
        }
        #[allow(dead_code)]
        impl $name {
            pub(crate) const fn new(key: &'static str, value: $value) -> Self {
                Self {
                    key,
                    default: value,
                    global: std::sync::atomic::$native::new(value),
                }
            }
            pub(crate) fn load(&self, order: Ordering) -> $value {
                STATE.with(|s| match s.borrow().as_ref() {
                    Some(m) => ($from)(*m.get(self.key).unwrap_or(&(self.default as u64))),
                    None => self.global.load(order),
                })
            }
            pub(crate) fn store(&self, value: $value, order: Ordering) {
                STATE.with(|s| match s.borrow_mut().as_mut() {
                    Some(m) => {
                        m.insert(self.key, value as u64);
                    }
                    None => self.global.store(value, order),
                });
            }
            pub(crate) fn swap(&self, value: $value, order: Ordering) -> $value {
                STATE.with(|s| match s.borrow_mut().as_mut() {
                    Some(m) => ($from)(
                        m.insert(self.key, value as u64)
                            .unwrap_or(self.default as u64),
                    ),
                    None => self.global.swap(value, order),
                })
            }
        }
    };
}
scoped!(AtomicBool, AtomicBool, bool, |v: u64| v != 0);
scoped!(AtomicU8, AtomicU8, u8, |v: u64| v as u8);
scoped!(AtomicU64, AtomicU64, u64, |v: u64| v);
impl AtomicU64 {
    pub(crate) fn fetch_add(&self, value: u64, order: Ordering) -> u64 {
        STATE.with(|s| match s.borrow_mut().as_mut() {
            Some(m) => {
                let entry = m.entry(self.key).or_insert(self.default);
                let old = *entry;
                *entry = entry.wrapping_add(value);
                old
            }
            None => self.global.fetch_add(value, order),
        })
    }
}

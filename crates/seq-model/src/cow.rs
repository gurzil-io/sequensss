//! Copy-on-write pointer for the heavy parts of the project.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Shared value, cloned only when modified ([`CowArc::make_mut`]). Cloning a project
/// copies pointers, so undo snapshots share everything that did not change.
///
/// Equality compares pointers first: two snapshots that share data compare in
/// O(number of parts), not O(notes). Serialized as a plain `T` (same file format).
#[derive(Default)]
pub struct CowArc<T>(Arc<T>);

impl<T> CowArc<T> {
    pub fn new(value: T) -> Self {
        CowArc(Arc::new(value))
    }

    /// Same allocation: same value, without comparing it.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// Address of the shared value (identity while a clone is kept alive).
    pub fn as_ptr(this: &Self) -> *const T {
        Arc::as_ptr(&this.0)
    }
}

impl<T: Clone> CowArc<T> {
    /// Mutable access; copies the value first if it is shared.
    pub fn make_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> Clone for CowArc<T> {
    fn clone(&self) -> Self {
        CowArc(self.0.clone())
    }
}

impl<T> Deref for CowArc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> From<T> for CowArc<T> {
    fn from(value: T) -> Self {
        CowArc::new(value)
    }
}

impl<T: PartialEq> PartialEq for CowArc<T> {
    fn eq(&self, other: &Self) -> bool {
        CowArc::ptr_eq(self, other) || *self.0 == *other.0
    }
}

impl<T: Eq> Eq for CowArc<T> {}

impl<T: fmt::Debug> fmt::Debug for CowArc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: Serialize> Serialize for CowArc<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CowArc<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(CowArc::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_shares_until_modified() {
        let a = CowArc::new(vec![1, 2, 3]);
        let mut b = a.clone();
        assert!(CowArc::ptr_eq(&a, &b));
        b.make_mut().push(4);
        assert!(!CowArc::ptr_eq(&a, &b));
        assert_eq!(*a, vec![1, 2, 3]);
        assert_eq!(*b, vec![1, 2, 3, 4]);
        // Unique: modified in place.
        let p = CowArc::as_ptr(&b);
        b.make_mut().pop();
        assert_eq!(p, CowArc::as_ptr(&b));
        // Different allocations, same value.
        assert_eq!(a, b);
    }
}

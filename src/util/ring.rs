//! A fixed-capacity ring buffer that drops the oldest entries.

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Ring<T> {
    items: VecDeque<T>,
    cap: usize,
}

impl<T> Ring<T> {
    pub fn new(cap: usize) -> Self {
        Self { items: VecDeque::with_capacity(cap.min(1024)), cap: cap.max(1) }
    }

    pub fn push(&mut self, item: T) {
        if self.items.len() == self.cap {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.items.iter()
    }

    pub fn last(&self) -> Option<&T> {
        self.items.back()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl<T: Clone> Ring<T> {
    /// The newest `n` entries in chronological order.
    pub fn tail(&self, n: usize) -> Vec<T> {
        let skip = self.items.len().saturating_sub(n);
        self.items.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_oldest() {
        let mut r = Ring::new(3);
        for i in 0..5 {
            r.push(i);
        }
        assert_eq!(r.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4]);
        assert_eq!(r.tail(2), vec![3, 4]);
        assert_eq!(r.tail(10), vec![2, 3, 4]);
    }
}

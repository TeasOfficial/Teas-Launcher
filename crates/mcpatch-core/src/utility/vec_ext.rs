//! Vec 扩展方法

/// 按条件删除元素的扩展 trait
pub trait VecRemoveIf<T> {
    fn remove_if(&mut self, predicate: impl Fn(&T) -> bool);
}

impl<T> VecRemoveIf<T> for Vec<T> {
    fn remove_if(&mut self, predicate: impl Fn(&T) -> bool) {
        let mut i = 0;
        while i < self.len() {
            if predicate(&self[i]) {
                self.remove(i);
            } else {
                i += 1;
            }
        }
    }
}

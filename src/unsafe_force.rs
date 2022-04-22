#[derive(Debug)]
pub struct Send<T>(T);

#[allow(clippy::missing_safety_doc)]
impl<T> Send<T> {
	pub unsafe fn new(t: T) -> Self {
		Send(t)
	}

	pub unsafe fn unwrap(self) -> T {
		self.0
	}
}

unsafe impl<T> std::marker::Send for Send<T> {}

pub struct Send<T>(T);

impl<T> Send<T> {
	pub unsafe fn new(t: T) -> Self {
		Send(t)
	}

	pub unsafe fn unwrap(self) -> T {
		self.0
	}
}

unsafe impl<T> std::marker::Send for Send<T> {}

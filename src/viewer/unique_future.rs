use futures::future::{abortable, AbortHandle, Aborted};
use std::{cell::Cell, future::Future, marker::PhantomData};

pub struct ReplaceIdentifier<T> {
	abort_handle: Cell<Option<AbortHandle>>,
	_phantom_data: PhantomData<T>,
}

impl<T> ReplaceIdentifier<T> {
	pub const fn new() -> Self {
		ReplaceIdentifier {
			abort_handle: Cell::new(None),
			_phantom_data: PhantomData,
		}
	}

	pub fn make_replaceable<Fut>(
		&self,
		future: Fut,
	) -> impl Future<Output = Result<Fut::Output, Aborted>>
	where
		Fut: Future<Output = T>,
	{
		let (future, abort_handle) = abortable(future);
		if let Some(handle_old_future) = self.abort_handle.replace(Some(abort_handle)) {
			handle_old_future.abort();
		}
		future
	}
}

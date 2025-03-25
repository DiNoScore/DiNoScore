use dinoscore::{library::ScaleMode, prelude::*, *};

use std::sync::mpsc::*;

glib::wrapper! {
	pub struct SongWidget(ObjectSubclass<imp::SongWidget>)
		@extends gtk::Widget,
		@implements gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager,
					adw::Swipeable;
}

impl SongWidget {
	pub fn load_song(
		&self,
		song: &Arc<collection::SongMeta>,
		pages: Arc<TiVec<collection::PageIndex, PageImage>>,
		mode: ScaleMode,
	) {
		let rendered_pages = Rc::new(
			std::iter::repeat(Default::default())
				.take(pages.len())
				.collect(),
		);

		let (renderer, mut update_page) = spawn_song_renderer(
			pages,
			song.version_uuid,
			song.piece_starts
				.keys()
				.map(|&staff| song.staves[staff].page)
				.collect(),
		);

		glib::MainContext::default().spawn_local(
			clone!(@weak self as obj => @default-panic, async move {
				use futures::StreamExt;
				while let Some(update_page) = update_page.next().await {
					obj.imp().update_page(update_page);
				}
			})
		);

		let mut state = SongState::new(
			renderer,
			song.clone(),
			rendered_pages,
			self.width() as f64,
			self.height() as f64,
			mode
		);
		self.imp().change_page(&mut state, 0.into());
		*self.imp().state.borrow_mut() = Some(state);
	}

	pub fn unload_song(&self) {
		self.imp().get_action("next-page").set_enabled(false);
		self.imp().get_action("previous-page").set_enabled(false);
		self.imp().get_action("next-piece").set_enabled(false);
		self.imp().get_action("previous-piece").set_enabled(false);
		*self.imp().state.borrow_mut() = None;
	}

	pub fn next_page(&self) {
		self.activate_action("song.next-page", None).unwrap();
	}

	pub fn previous_page(&self) {
		self.activate_action("song.previous-page", None).unwrap();
	}

	pub fn next_piece(&self) {
		self.activate_action("song.next-piece", None).unwrap();
	}

	pub fn previous_piece(&self) {
		self.activate_action("song.previous-piece", None).unwrap();
	}
}

mod imp {
	use super::*;

	#[derive(Default)]
	pub struct SongWidget {
		pub(super) state: RefCell<Option<SongState>>,
		pub(super) scroll_animation: OnceCell<adw::SpringAnimation>,
		pub actions: gio::SimpleActionGroup,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongWidget {
		const NAME: &'static str = "ViewerSongWidget";
		type Type = super::SongWidget;
		type ParentType = gtk::Widget;
		// type Interfaces = (adw::Swipeable,);

		fn class_init(_klass: &mut Self::Class) {}

		fn instance_init(_obj: &InitializingObject<Self>) {}
	}

	impl ObjectImpl for SongWidget {
		fn signals() -> &'static [glib::subclass::Signal] {
			use glib::subclass::Signal;
			Box::leak(Box::new([
				Signal::builder("progress")
					.param_types([f64::static_type()])
					.build(),
			]))
		}

		fn properties() -> &'static [glib::ParamSpec] {
			Box::leak(Box::new([
				glib::ParamSpecUInt::builder("part-index")
					.nick("part-index")
					.blurb("part")
					.default_value(gtk::INVALID_LIST_POSITION)
					.flags(glib::ParamFlags::READWRITE)
					.build(),
			]))
		}

		fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
			match pspec.name() {
				"part-index" => self
					.state
					.borrow()
					.as_ref()
					.map(|state| (state.current_piece_index() as u32).to_value())
					.expect("TODO let's see if this ever happens"),
				_ => unimplemented!(),
			}
		}

		fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
			match pspec.name() {
				"part-index" => {
					let mut state = self.state.borrow_mut();
					let Some(state) = state.as_mut() else { return; };
					let part = value.get::<u32>().unwrap();
					if part == gtk::INVALID_LIST_POSITION {
						return;
					}
					self.change_position(
						state,
						*state.song.piece_starts.keys()
							.nth(part as usize)
							.expect("`part-index` out of bounds")
					);
				}
				_ => unimplemented!(),
			}
		}

		fn constructed(&self) {
			self.parent_constructed();
			let obj = self.obj();

			let actions = &self.actions;

			/* Actions are declared here but registered on the SongPane */
			self.actions.add_action_entries([
				gio::ActionEntry::builder("next-page")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().next_page();
					}))
					.build(),
					gio::ActionEntry::builder("previous-page")
						.activate(clone_!(self, move |obj, _g, _a, _p| {
							obj.imp().previous_page();
						}))
						.build(),
				gio::ActionEntry::builder("next-piece")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().next_piece();
					}))
					.build(),
				gio::ActionEntry::builder("previous-piece")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().previous_piece();
					}))
					.build(),
				gio::ActionEntry::builder("sizing-mode")
					.parameter_type(Some(&String::static_variant_type()))
					.state("manual".to_variant())
					.activate(clone_!(self, move |obj, _g, _a, p| {
						// obj.imp().scale_mode_changed(p.unwrap().clone());
					}))
					.build(),
			]);

			let target = adw::CallbackAnimationTarget::new(glib::clone!(
				@weak obj => @default-panic,
				move |position| log::debug!("Animation pos: {position}")
			));

			/* Zero mass, high friction, critically damped => no oscillation or overswinging */
			const SCROLL_DAMPING_RATIO: f64 = 1.0;
			const SCROLL_MASS: f64 = 0.0;
			const SCROLL_STIFFNESS: f64 = 500.0;

			let animation = adw::SpringAnimation::builder()
				// .spring_params(&adw::SpringParams::new(
				// 	SCROLL_DAMPING_RATIO,
				// 	SCROLL_MASS,
				// 	SCROLL_STIFFNESS,
				// ))
				.widget(&*obj)
				.target(&target)
				.value_to(0.0)
				.build();

			animation.connect_done(glib::clone!(
				@weak obj => @default-panic,
				move |_| log::debug!("Animation end")
			));
			self.scroll_animation.set(animation).unwrap();
		}
	}

	impl WidgetImpl for SongWidget {
		fn snapshot(&self, snapshot: &gtk::Snapshot) {
			self.parent_snapshot(snapshot);
			let obj = self.obj();

			/* Zero sizes cause problems */
			if obj.width() < 1 || obj.height() < 1 {
				return;
			}

			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else { return; };

			/* We recalculate the layout on the fly during rendering if our size changed.
			 * This means that we must keep everything else constant (not trigger any signals).
			 */
			state.check_size(obj.width(), obj.height());

			let layout = &state.layout;

			// TODO remove because that's what the overflow property is there for
			let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
			/* Make sure we don't render outside of out widget */
			snapshot.push_clip(&bounds);

			/* The actual rendering code. Might be called twice for dark mode */
			let render = || {
				snapshot.append_color(&gdk::RGBA::WHITE, &bounds);
				layout.get_page_of_staff(state.position).1
					.iter()
					.try_for_each(|staff_layout| {
						snapshot.save();
						/* Point origin at staff start */
						snapshot.translate(&graphene::Point::new(
							staff_layout.x as f32,
							staff_layout.y as f32,
						));

						/* Staff */
						snapshot.save();
						let staff = &state.song.staves[staff_layout.index];
						let (rendered_page, annotations) =
							&*state.rendered_pages[staff.page].borrow();
						match rendered_page.as_ref() {
							Some(page) => {
								/* Render the image */
								snapshot.push_clip(&graphene::Rect::new(
									0.0,
									0.0,
									staff_layout.width as f32,
									staff_layout.width as f32 * staff.aspect_ratio() as f32,
								));
								let scale = staff_layout.width as f32 / staff.width() as f32;
								snapshot.scale(scale, scale);
								snapshot.append_texture(
									page,
									&graphene::Rect::new(
										-staff.start.0 as f32,
										-staff.start.1 as f32,
										1.0,
										page.height() as f32 / page.width() as f32,
									),
								);
								snapshot.pop();
							},
							None => {
								/* Render a placeholder */
								snapshot.append_color(
									&gdk::RGBA::new(0.8, 0.8, 0.8, 1.0),
									&graphene::Rect::new(
										0.0,
										0.0,
										staff_layout.width as f32,
										staff_layout.width as f32 * staff.aspect_ratio() as f32,
									),
								);
							},
						}
						snapshot.restore();

						/* Page/Staff number */
						let context = snapshot.append_cairo(&bounds);
						context.set_font_size(20.0);
						context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
						context.move_to(10.0, 16.0);
						let (page_index, staff_index) = state.song.page_of_piece(staff_layout.index);
						context.show_text(&format!("{}-{}", *page_index + 1, *staff_index))?;

						snapshot.restore();

						/* Render annotations */
						if let Some(page) = annotations.as_ref() {
							let context = snapshot.append_cairo(&bounds);

							context.translate(staff_layout.x, staff_layout.y);

							let scale = staff_layout.width / staff.width();
							context.scale(scale, scale);
							context.translate(-staff.start.0, -staff.start.1);

							context.rectangle(
								staff.start.0,
								staff.start.1,
								staff.width(),
								staff.height(),
							);
							context.clip();

							context.scale(1.0 / page.size().0, 1.0 / page.size().0);
							page.render(&context);
						}

						cairo::Result::Ok(())
					})
					.expect("Failed to draw");
			};

			if adw::StyleManager::default().is_dark() {
				/* Dark mode: Invert luminosity by inverting colors + blending */
				snapshot.push_blend(gsk::BlendMode::Luminosity);
				render();
				snapshot.pop();
				snapshot.push_color_matrix(
					&graphene::Matrix::new_scale(-1.0, -1.0, -1.0),
					&graphene::Vec4::one(),
				);
				render();
				snapshot.pop();
				snapshot.pop();
			} else {
				render();
			}

			snapshot.pop();
		}
	}

	impl SongWidget {
		/// The background thread has finished rendering some page
		pub(super) fn update_page(&self, update_page: ScaledPage) {
			let mut state = self.state.borrow_mut();
			let state = state.as_mut().unwrap();

			/* Check for stale data (probably wouldn't have to with the current design, but it may change in the future */
			log::debug!("Received page");
			if state.song.version_uuid != update_page.song {
				log::debug!("Ignoring incoming rendered pages because it's stale (song changed)");
				return;
			}

			self.obj().emit_by_name::<()>("progress", &[&update_page.progress]);

			(*state.rendered_pages[update_page.index].borrow_mut()).0 = Some(update_page.image);
			self.obj().queue_draw();
		}

		pub(super) fn get_action(&self, name: &str) -> gio::SimpleAction {
			self.actions.lookup_action(name).unwrap()
				.downcast::<gio::SimpleAction>()
				.unwrap()
		}

		pub fn next_page(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else { return; };
			self.change_page(state, state.layout.get_page_of_staff(state.position).0 + layout::PageIndex(1));
		}

		pub fn previous_page(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else { return; };
			self.change_position(
				state,
				state.section_start().unwrap_or(
					state.layout.pages[state.layout.get_page_of_staff(state.position).0 - layout::PageIndex(1)][0].index
				)
			);
		}

		/// Go to the beginning of the next piece
		pub fn next_piece(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else { return; };

			self.change_position(state, state.next_piece().expect("This action should have been disabled"));
		}

		/// Go to beginning of the current or previous piece
		pub fn previous_piece(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else { return; };
			self.change_position(state, state.previous_piece());
		}

		pub(super) fn change_page(&self, state: &mut SongState, page: layout::PageIndex) {
			self.change_position(
				state,
				state.layout.pages[page]
					.first()
					.expect("Every page must have at least one Staff")
					.index
			);
		}

		pub(super) fn change_position(&self, state: &mut SongState, position: collection::StaffIndex) {
			let (page, _) = state.layout.get_page_of_staff(position);
			log::debug!("Changing page {page} (staff {position})");
			let old_part = state.current_piece_index();

			self.scroll_animation.get().unwrap().set_value_from(*state.position as f64 - *position as f64);
			self.scroll_animation.get().unwrap().play();
			state.position = position;

			self.get_action("next-page").set_enabled(*page < state.layout.pages.len() - 1);
			self.get_action("previous-page").set_enabled(*page > 0);
			self.get_action("next-piece").set_enabled(state.next_piece().is_some());
			self.get_action("previous-piece").set_enabled(*page > 0);

			if state.current_piece_index() != old_part {
				// TODO this is a recipe for disaster
				let obj = self.obj().clone();
				glib::spawn_future_local(async move {
					obj.notify("part-index");
				});
			}

			/* Notify background renderer about potential changes */
			state.renderer
				.send((
					/* Convert current layout page to PDF page */
					state.song.staves[position].page,
					None,
				))
				.unwrap();

			self.obj().queue_draw();
			self.obj().grab_focus();
		}
	}
}

pub(self) struct SongState {
	song: Arc<collection::SongMeta>,
	rendered_pages: Rc<
		TiVec<
			collection::PageIndex,
			/* page, annotations */
			RefCell<(Option<gdk::Texture>, Option<poppler::Page>)>,
		>,
	>,
	/* Instead of storing the current page, the position is stored as the page containing a certain staff.
	 * This makes it invariant to layout changes.
	 * After a page change, this points to the first staff of a page by convention, but after
	 * a resize things might be shuffled around.
	 */
	position: collection::StaffIndex,
	layout: layout::PageLayout,
	renderer: Sender<(collection::PageIndex, Option<i32>)>,
	zoom: f64,
	scale_mode: ScaleMode,
	/* Offset when animating between pages. Range -1.0..1.0 for previous/next page. */
	render_offset: f64,
	/* Backup for when a gesture starts */
	zoom_before_gesture: Option<f64>,
}

impl SongState {
	fn new(
		renderer: Sender<(collection::PageIndex, Option<i32>)>,
		song: Arc<collection::SongMeta>,
		rendered_pages: Rc<
			TiVec<
				collection::PageIndex,
				/* page, annotations */
				RefCell<(Option<gdk::Texture>, Option<poppler::Page>)>,
			>,
		>,
		width: f64,
		height: f64,
		scale_mode: ScaleMode,
	) -> Self {
		// let layout = Arc::new(layout::layout_fixed_width(&song, width, height, 1.0, 10.0));
		// let layout = Arc::new(layout::layout_fixed_height(&song, width, height));
		let layout = layout::layout_fixed_scale(&song, width, height, 1.0);
		Self {
			song,
			rendered_pages,
			position: 0.into(),
			layout,
			renderer,
			zoom: 1.0,
			scale_mode,
			zoom_before_gesture: None,
			render_offset: 0.0,
		}
	}

	fn check_size(&mut self, width: i32, height: i32) {
		if width != self.layout.width as i32 || height != self.layout.height as i32 {
			self.change_size(width as f64, height as f64);
		}
	}

	fn change_size(&mut self, width: f64, height: f64) {
		// self.layout = Arc::new(layout::layout_fixed_width(&self.song, width, height, zoom, 10.0));
		// self.layout = Arc::new(layout::layout_fixed_height(&self.song, width, height));
		match self.scale_mode {
			ScaleMode::Zoom(_) => {},
			ScaleMode::FitStaves(num) => {
				self.zoom = layout::find_scale_for_fixed_staves(&self.song, width, height, num)
			},
			ScaleMode::FitPages(num) => {
				self.zoom = layout::find_scale_for_fixed_columns(&self.song, width, height, num)
			},
		}

		self.layout = layout::layout_fixed_scale(
			&self.song, width, height, self.zoom,
		);

		/* Calculate the maximum effective page width for this layout */
		use noisy_float::prelude::*;
		let render_width: f64 = self
			.layout
			.pages
			.iter()
			.flatten()
			.map(|layout_staff| {
				r64(layout_staff.width / self.song.staves[layout_staff.index].width())
			})
			.max()
			.unwrap_or_default()
			.into();

		/* Notify background renderer about potential changes */
		self.renderer
			.send((
				/* Convert current layout page to PDF page */
				self.song.staves[self.position].page,
				Some(render_width as i32),
			))
			.unwrap();
	}

	/* On which staff does our current page start? */
	fn page_start(&self) -> collection::StaffIndex {
		self.layout.get_page_of_staff(self.position).1[0].index
	}

	fn page_end(&self) -> collection::StaffIndex {
		self.layout.get_page_of_staff(self.position).1.last().unwrap().index
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition?
	 * Returns `None` if there are no repetitions and we should simply go back one page instead.
	 */
	fn section_start(&self) -> Option<collection::StaffIndex> {
		/* Find all sections that are repetitions and are visible on the current page.
		 * Go back to the beginning of the first of them.
		 */
		let page_start = self.page_start();
		self.song.section_starts
			.range(..page_start)
			.next_back()
			.filter(|(_, meta)| meta.is_repetition)
			// TODO Make sure the section also is at least somewhat visible on the current page
			.map(|(idx, _)| *idx)
	}

	fn current_piece_index(&self) -> usize {
		self.song.piece_starts.range(..=self.position).count() - 1
	}

	/* When we're at a given position, where did the part we are in start? */
	fn previous_piece(&self) -> collection::StaffIndex {
		self.song
			.piece_starts
			.range(..self.page_start())
			.next_back()
			.map(|(i, _)| *i)
			.unwrap_or_else(|| 0.into())
	}

	/* Returns `None` if we already are in the last piece */
	fn next_piece(&self) -> Option<collection::StaffIndex> {
		use std::ops::Bound;
		self.song
			.piece_starts
			.range((Bound::Excluded(self.page_end()), Bound::Unbounded))
			.next()
			.map(|(i, _)| *i)
	}
}

/// A pre-rasterized page
#[derive(Debug)]
struct ScaledPage {
	index: collection::PageIndex,
	image: gdk::Texture,
	/* To filter out old/stale values */
	song: uuid::Uuid,
	progress: f64,
}

/// A background thread renderer
///
/// It will take the raw PDFs and images and render them scaled down to an appropriate
/// size. It is flexible with in-flight requests and invalidation.
///
/// Drop one of the channels when you are no longer interested in that song.
fn spawn_song_renderer(
	pages: Arc<TiVec<collection::PageIndex, PageImage>>,
	song: uuid::Uuid,
	mut piece_starts: Vec<collection::PageIndex>,
) -> (
	Sender<(collection::PageIndex, Option<i32>)>,
	futures::channel::mpsc::UnboundedReceiver<ScaledPage>,
) {
	/* Sometimes, two pieces start on the same page. Irrelevant for our purposes */
	piece_starts.dedup();

	let (in_tx, in_rx) = channel();
	let (out_tx, out_rx) = futures::channel::mpsc::unbounded();

	std::thread::spawn(move || {
		use std::collections::VecDeque;
		/* This used to create a simple list of all staves in order.
		 * Except for the initial load, the order does not matter, since
		 * the queue is reordered according to the currently visible page.
		 * Here, we interleave the pages across different parts so that
		 * the users gets a quick initial response, even when jumping directly
		 * to one of the later sections of the song.
		 */
		let reset_work_queue = || {
			let mut piece_starts: Vec<std::ops::Range<collection::PageIndex>> = piece_starts
				.windows(2)
				.map(|win| (win[0], win[1]))
				.map(|(start, end)| start..end)
				.chain(std::iter::once(
					piece_starts[piece_starts.len() - 1]..collection::PageIndex(pages.len()),
				))
				.collect();
			let mut work_queue = VecDeque::with_capacity(pages.len());
			while !piece_starts.is_empty() {
				for piece in &mut piece_starts {
					work_queue.push_back(piece.start);
					piece.start += collection::PageIndex(1);
				}
				piece_starts.retain(|r| !r.is_empty());
			}
			assert_eq!(work_queue.len(), pages.len());
			work_queue
		};

		/* For a start, render everything sequentially at minimum resolution. This should not take long */
		let start = std::time::Instant::now();
		for i in reset_work_queue() {
			let image = gdk::Texture::for_pixbuf(&pages[i].render_scaled(250));
			if out_tx
				.unbounded_send(ScaledPage {
					index: i,
					image,
					song,
					progress: i.0 as f64 / pages.len() as f64 / pages.len() as f64,
				})
				.is_err()
			{
				return;
			}

			/* Limit the initial step to one second. Otherwise it will take too long to render the first
			 * full resolution image
			 */
			if start.elapsed() > std::time::Duration::from_secs(1) {
				break;
			}
		}
		log::debug!("Background renderer ready");

		/* Start with empty queue since we just already did that resolution */
		let mut work_queue = VecDeque::default();
		let mut work_width = 250;
		let mut work_page = collection::PageIndex(0);

		/* We always only want the latest value */
		type Update = (collection::PageIndex, Option<i32>);
		fn fetch_latest(rx: &Receiver<Update>, block: bool) -> Result<Option<Update>, ()> {
			let mut last = None::<Update>;
			loop {
				match rx.try_recv() {
					Ok((page, None)) if last.is_some() => {
						last = Some((page, last.unwrap().1));
					},
					Ok(val) => {
						last = Some(val);
					},
					Err(TryRecvError::Empty) if last.is_none() && block => {
						/* Don't return empty handed */
						return rx.recv().map(Option::Some).map_err(|_| ());
					},
					Err(TryRecvError::Empty) => return Ok(last),
					Err(TryRecvError::Disconnected) => return Err(()),
				}
			}
		}

		loop {
			let mut need_queue_shuffle = false;

			/* If we have work to do, we simply check for potential invalidation. If we're idle, block on new work */
			match fetch_latest(&in_rx, work_queue.is_empty()) {
				Ok(Some((page, width))) => {
					/* Change for width changes */
					if let Some(width) = width {
						/* Round the width to the nearest level. Never round down, never round more than
						 * 66% (the levels are 2/3 apart each, exponentially). Never go below 250 pixels.
						 */
						let actual_width = (1.5f64)
							.powf((width as f64).log(1.5).ceil())
							.ceil()
							.max(250.0) as i32;
						if actual_width != work_width {
							log::debug!(
								"Background thread rendering width changed: {actual_width}"
							);
							work_width = actual_width;
							work_queue = reset_work_queue();
							need_queue_shuffle = true;
						}
					}

					/* Check for page changes, update work queue accordingly */
					if page != work_page {
						work_page = page;
						need_queue_shuffle = true;
					}
				},
				Ok(None) => (),
				Err(_) => return,
			}

			/* Update queue based on distance to the current page */
			if need_queue_shuffle && !work_queue.is_empty() {
				log::debug!("Priority page change: {work_page}");
				work_queue
					.make_contiguous()
					.sort_unstable_by_key(|page| (**page as isize - *work_page as isize).abs());
			}

			if let Some(page) = work_queue.pop_front() {
				/* Now we can finally do some work */
				let image = gdk::Texture::for_pixbuf(&pages[page].render_scaled(work_width));

				/* Send it off */
				if out_tx
					.unbounded_send(ScaledPage {
						index: page,
						image,
						song,
						progress: (pages.len() - work_queue.len()) as f64 / pages.len() as f64,
					})
					.is_err()
				{
					return;
				}
			}
		}
	});

	(in_tx, out_rx)
}

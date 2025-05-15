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
	pub fn get_scale_mode(&self) -> ScaleMode {
		let scale_mode = self
			.imp()
			.get_action("sizing-mode")
			.state()
			.unwrap()
			.get::<String>()
			.unwrap();
		match &*scale_mode {
			"fit-staves" => ScaleMode::FitStaves(3),
			"fit-columns" => ScaleMode::FitPages(2),
			"manual" => ScaleMode::Zoom(self.zoom()),
			other => panic!("Invalid value for `scale-mode` '{}'", other),
		}
	}

	pub fn set_scale_mode(&self, scale_mode: ScaleMode) {
		let action = self.imp().get_action("sizing-mode");
		match scale_mode {
			ScaleMode::FitStaves(_) => action.activate(Some(&"fit-staves".to_variant())),
			ScaleMode::FitPages(_) => action.activate(Some(&"fit-columns".to_variant())),
			ScaleMode::Zoom(zoom) => self.set_zoom(zoom),
		}
	}

	pub fn load_song(
		&self,
		song: &Arc<collection::SongMeta>,
		pages: Arc<TiVec<collection::PageIndex, PageImage>>,
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

		glib::MainContext::default().spawn_local(clone!(
			#[weak(rename_to = obj)]
			self,
			#[upgrade_or_panic]
			async move {
				use futures::StreamExt;
				while let Some(update_page) = update_page.next().await {
					obj.imp().update_page(update_page);
				}
			}
		));

		let state = SongState::new(
			renderer,
			song.clone(),
			rendered_pages,
			self.width() as f64,
			self.height() as f64,
		);
		*self.imp().state.borrow_mut() = Some(state);
		self.imp().restart_cursor_timer();
	}

	pub fn unload_song(&self) {
		self.imp().get_action("next-page").set_enabled(false);
		self.imp().get_action("previous-page").set_enabled(false);
		self.imp().get_action("next-piece").set_enabled(false);
		self.imp().get_action("previous-piece").set_enabled(false);
		*self.imp().state.borrow_mut() = None;
	}

	pub fn load_annotations(&self, annotations: Option<poppler::Document>) {
		let mut state = self.imp().state.borrow_mut();
		let Some(state) = state.as_mut() else {
			return;
		};

		if let Some(annotations) = annotations {
			assert_eq!(
				annotations.n_pages() as usize,
				state.rendered_pages.len(),
				"Annotation document must have as many pages as original PDF"
			);

			for i in 0..state.rendered_pages.len() {
				(*state.rendered_pages[collection::PageIndex(i)].borrow_mut()).1 =
					annotations.page(i as i32);
			}
		} else {
			state
				.rendered_pages
				.iter()
				.for_each(|page| page.borrow_mut().1 = None);
		}
		self.queue_draw();
	}

	pub fn next_page(&self) {
		if self.imp().get_action("next-page").is_enabled() {
			self.imp().next_page();
		}
	}

	/* Go back one page or until the repetition */
	pub fn previous_page(&self) {
		if self.imp().get_action("previous-page").is_enabled() {
			self.imp().previous_page();
		}
	}

	/* Only go back exactly one page */
	pub fn previous_page_strict(&self) {
		if self.imp().get_action("previous-page").is_enabled() {
			self.imp().previous_page_strict();
		}
	}

	pub fn next_piece(&self) {
		if self.imp().get_action("next-piece").is_enabled() {
			self.imp().next_piece();
		}
	}

	pub fn previous_piece(&self) {
		if self.imp().get_action("previous-piece").is_enabled() {
			self.imp().previous_piece();
		}
	}
}

mod imp {
	use super::*;

	#[derive(Default, Properties, CompositeTemplate)]
	#[template(resource = "/de/piegames/dinoscore/viewer/song_widget.ui")]
	#[properties(wrapper_type = super::SongWidget)]
	pub struct SongWidget {
		pub(super) state: RefCell<Option<SongState>>,
		#[property(
			get = |obj: &&SongWidget| obj.get_part_index(),
			set = |obj: &&SongWidget, val| obj.set_part_index(val))
		]
		part_index: std::marker::PhantomData<u32>,
		pub(super) scroll_animation: OnceCell<adw::SpringAnimation>,
		pub(super) swipe_tracker: OnceCell<adw::SwipeTracker>,
		pub actions: gio::SimpleActionGroup,
		/* Rendering offset, for swiping and animations */
		pub(super) offset: Cell<f64>,
		#[property(get, set =  |obj: &&SongWidget, val| obj.set_zoom(val), construct, default = 1.0)]
		zoom: Cell<f64>,
		/* Backup for when a gesture starts. Always Some during a gesture */
		zoom_before_gesture: Cell<Option<f64>>,
		/* Automatically hide the cursor after some seconds of inactivity */
		hide_cursor: RefCell<Option<glib::source::SourceId>>,
		#[template_child]
		scroll_gesture: TemplateChild<gtk::EventControllerScroll>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongWidget {
		const NAME: &'static str = "ViewerSongWidget";
		type Type = super::SongWidget;
		type ParentType = gtk::Widget;
		type Interfaces = (adw::Swipeable,);

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	#[glib::derived_properties]
	impl ObjectImpl for SongWidget {
		fn signals() -> &'static [glib::subclass::Signal] {
			use glib::subclass::Signal;
			Box::leak(Box::new([
				Signal::builder("progress")
					.param_types([f64::static_type()])
					.build(),
				/* Changing a page counts as "activity", changing zoom etc. does not
				 * For simplicity, page changes induced by recize also count as activity, even though
				 * one may consider them as false positive
				 */
				Signal::builder("activity")
					.param_types([] as [glib::subclass::SignalType; 0])
					.build(),
			]))
		}

		fn constructed(&self) {
			self.parent_constructed();
			let obj = self.obj();

			/* Actions are declared here but registered on the SongPane */
			self.actions.add_action_entries([
				gio::ActionEntry::builder("next-page")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.next_page();
					}))
					.build(),
				gio::ActionEntry::builder("previous-page")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.previous_page();
					}))
					.build(),
				gio::ActionEntry::builder("next-piece")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.next_piece();
					}))
					.build(),
				gio::ActionEntry::builder("previous-piece")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.previous_piece();
					}))
					.build(),
				gio::ActionEntry::builder("sizing-mode")
					.parameter_type(Some(&String::static_variant_type()))
					.state("manual".to_variant())
					.activate(clone_!(self, move |obj, _g, a, p| {
						a.set_state(p.unwrap());
						obj.imp().scale_mode_changed();
					}))
					.build(),
				gio::ActionEntry::builder("zoom-in")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().zoom_in();
					}))
					.build(),
				gio::ActionEntry::builder("zoom-out")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().zoom_out();
					}))
					.build(),
				gio::ActionEntry::builder("zoom-original")
					.activate(clone_!(self, move |obj, _g, _a, _p| {
						obj.imp().zoom_reset();
					}))
					.build(),
			]);

			let swipe_tracker = adw::SwipeTracker::builder()
				.allow_mouse_drag(true)
				.allow_long_swipes(false)
				.swipeable(&*obj)
				.orientation(gtk::Orientation::Vertical)
				.build();
			swipe_tracker.connect_begin_swipe(glib::clone!(
				#[weak]
				obj,
				move |_| obj.imp().scroll_animation.get().unwrap().pause()
			));
			swipe_tracker.connect_update_swipe(glib::clone!(
				#[weak]
				obj,
				move |_, position| {
					let state = obj.imp().state.borrow();
					let Some(state) = state.as_ref() else {
						return;
					};
					obj.imp()
						.offset
						.set(position - *state.layout.get_page_of_staff(state.position).0 as f64);
					obj.queue_draw();
				}
			));
			swipe_tracker.connect_end_swipe(glib::clone!(
				#[weak]
				obj,
				move |_, velocity, to| {
					let this = obj.imp();
					let mut state = this.state.borrow_mut();
					let Some(state) = state.as_mut() else {
						return;
					};
					let new_pos = state
						.layout
						.get_staves_of_page(layout::PageIndex(to.round() as usize))
						.next()
						.unwrap();
					this.change_position(state, new_pos, this.offset.get(), velocity);
				}
			));
			self.swipe_tracker.set(swipe_tracker).unwrap();

			let target: libadwaita::CallbackAnimationTarget =
				adw::CallbackAnimationTarget::new(glib::clone!(
					#[weak]
					obj,
					#[upgrade_or_panic]
					move |offset| {
						obj.imp().offset.set(offset);
						obj.queue_draw();
					}
				));

			/* Same as in Loupe and AdwCarousel */
			const SCROLL_DAMPING_RATIO: f64 = 1.0; /* Perfectly damped */
			const SCROLL_MASS: f64 = 0.5;
			const SCROLL_STIFFNESS: f64 = 500.0;
			let animation = adw::SpringAnimation::builder()
				.value_to(0.0)
				.spring_params(&adw::SpringParams::new(
					SCROLL_DAMPING_RATIO,
					SCROLL_MASS,
					SCROLL_STIFFNESS,
				))
				.widget(&*obj)
				.target(&target)
				.build();

			animation.connect_done(glib::clone!(
				#[weak]
				obj,
				#[upgrade_or_panic]
				move |_| obj.queue_draw()
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
			let Some(state) = state.as_mut() else {
				return;
			};

			/* We recalculate the layout on the fly during rendering if our size changed.
			 * This means that we must keep everything else constant (not trigger any signals).
			 */
			if obj.width() != state.layout.width as i32
				|| obj.height() != state.layout.height as i32
			{
				self.update_layout(state);
			}

			let layout = &state.layout;

			// TODO remove because that's what the overflow property is there for
			let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
			/* Make sure we don't render outside of out widget */
			snapshot.push_clip(&bounds);

			/* The actual rendering code. Might be called twice for dark mode */
			let render_staff = |staff_layout: &layout::StaffLayout| {
				snapshot.save();
				/* Point origin at staff start */
				snapshot.translate(&graphene::Point::new(
					staff_layout.x as f32,
					staff_layout.y as f32,
				));

				/* Staff */
				snapshot.save();
				let staff = &state.song.staves[staff_layout.index];
				let (rendered_page, annotations) = &*state.rendered_pages[staff.page].borrow();
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

					context.rectangle(staff.start.0, staff.start.1, staff.width(), staff.height());
					context.clip();

					context.scale(1.0 / page.size().0, 1.0 / page.size().0);
					page.render(&context);
				}

				cairo::Result::Ok(())
			};

			let render = || {
				/* Draw background */
				snapshot.append_color(&gdk::RGBA::WHITE, &bounds);

				let current_pos =
					*layout.get_page_of_staff(state.position).0 as f64 + self.offset.get();
				assert!((0.0..=layout.pages.len() as f64 - 1.0).contains(&current_pos));
				/* This range will contain only one item if we are exactly on a page */
				let pages = layout::PageIndex(current_pos.floor() as _)
					..=layout::PageIndex(current_pos.ceil() as _);

				let offset = current_pos.fract();
				for (idx, page) in layout.pages[pages].iter().enumerate() {
					snapshot.save();
					/* Make sure we don't overdraw our pages, which might cause clipping */
					snapshot.translate(&graphene::Point::new(
						0.0,
						(idx as f32 - offset as f32) * obj.height() as f32,
					));
					snapshot.push_clip(&bounds);
					page.iter()
						.try_for_each(render_staff)
						.expect("Failed to draw");
					snapshot.pop();
					snapshot.restore();
				}
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

	impl SwipeableImpl for SongWidget {
		fn distance(&self) -> f64 {
			match self.swipe_tracker.get().unwrap().orientation() {
				gtk::Orientation::Horizontal => self.obj().width() as f64,
				gtk::Orientation::Vertical => self.obj().height() as f64,
				_ => unimplemented!(),
			}
		}

		fn progress(&self) -> f64 {
			let state = self.state.borrow();
			let Some(state) = state.as_ref() else {
				return Default::default();
			};

			*state.layout.get_page_of_staff(state.position).0 as f64 + self.offset.get()
		}

		fn snap_points(&self) -> Vec<f64> {
			let state = self.state.borrow();
			let Some(state) = state.as_ref() else {
				return Default::default();
			};

			(0..state.layout.pages.len()).map(|i| i as f64).collect()
		}

		fn cancel_progress(&self) -> f64 {
			let snap_points = self.snap_points();
			/* Copied over from Loupe, no clue what it does */
			if let (Some(min), Some(max)) = (snap_points.first(), snap_points.last()) {
				self.progress().round().clamp(*min, *max)
			} else {
				0.0
			}
		}
	}

	#[gtk::template_callbacks]
	impl SongWidget {
		fn get_part_index(&self) -> u32 {
			self.state
				.borrow()
				.as_ref()
				.map(|state| state.current_piece_index() as u32)
				.unwrap_or_default()
		}

		fn set_part_index(&self, part: u32) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};
			if part == gtk::INVALID_LIST_POSITION {
				return;
			}
			self.change_position(
				state,
				*state
					.song
					.piece_starts
					.keys()
					.nth(part as usize)
					.expect("`part-index` out of bounds"),
				0.0,
				0.0,
			);
		}

		/// The background thread has finished rendering some page
		pub(super) fn update_page(&self, update_page: ScaledPage) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};

			/* Check for stale data (probably wouldn't have to with the current design, but it may change in the future */
			log::debug!("Received page");
			if state.song.version_uuid != update_page.song {
				log::debug!("Ignoring incoming rendered pages because it's stale (song changed)");
				return;
			}

			self.obj()
				.emit_by_name::<()>("progress", &[&update_page.progress]);

			(*state.rendered_pages[update_page.index].borrow_mut()).0 = Some(update_page.image);
			self.obj().queue_draw();
		}

		#[track_caller]
		pub(super) fn get_action(&self, name: &str) -> gio::SimpleAction {
			self.actions
				.lookup_action(name)
				.expect("Action not found")
				.downcast::<gio::SimpleAction>()
				.unwrap()
		}

		pub fn next_page(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};
			self.change_page(
				state,
				state.layout.get_page_of_staff(state.position).0 + layout::PageIndex(1),
			);
		}

		pub fn previous_page(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};
			self.change_position(
				state,
				state.section_start().unwrap_or(
					state.layout.pages
						[state.layout.get_page_of_staff(state.position).0 - layout::PageIndex(1)][0]
						.index,
				),
				0.0,
				0.0,
			);
		}

		pub fn previous_page_strict(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};
			self.change_page(
				state,
				state.layout.get_page_of_staff(state.position).0 - layout::PageIndex(1),
			);
		}

		/// Go to the beginning of the next piece
		pub fn next_piece(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};

			self.change_position(
				state,
				state
					.next_piece()
					.expect("This action should have been disabled"),
				0.0,
				0.0,
			);
		}

		/// Go to beginning of the current or previous piece
		pub fn previous_piece(&self) {
			let mut state = self.state.borrow_mut();
			let Some(state) = state.as_mut() else {
				return;
			};
			self.change_position(state, state.previous_piece(), 0.0, 0.0);
		}

		pub(super) fn change_page(&self, state: &mut SongState, page: layout::PageIndex) {
			self.change_position(
				state,
				state.layout.pages[page]
					.first()
					.expect("Every page must have at least one Staff")
					.index,
				0.0,
				0.0,
			);
		}

		pub(super) fn change_position(
			&self,
			state: &mut SongState,
			position: collection::StaffIndex,
			animation_offset: f64,
			animation_velocity: f64,
		) {
			let (page, _) = state.layout.get_page_of_staff(position);
			let (old_page, _) = state.layout.get_page_of_staff(state.position);
			log::debug!("Changing page {page} (staff {position})");
			let old_part = state.current_piece_index();

			let animation = self.scroll_animation.get().unwrap();
			animation.set_value_from(*old_page as f64 - *page as f64 + animation_offset);
			animation.set_initial_velocity(animation_velocity);
			animation.play();
			state.position = position;

			self.get_action("next-page")
				.set_enabled(*page < state.layout.pages.len() - 1);
			self.get_action("previous-page").set_enabled(*page > 0);
			self.get_action("next-piece")
				.set_enabled(state.next_piece().is_some());
			self.get_action("previous-piece").set_enabled(*page > 0);

			if state.current_piece_index() != old_part {
				// TODO this is a recipe for disaster
				let obj = self.obj().clone();
				glib::spawn_future_local(async move {
					obj.notify("part-index");
				});
			}

			/* Notify background renderer about potential changes */
			state
				.renderer
				.send((
					/* Convert current layout page to PDF page */
					state.song.staves[position].page,
					None,
				))
				.unwrap();

			self.obj().queue_draw();
			self.obj().grab_focus();
			self.obj().emit_by_name::<()>("activity", &[]);
		}

		fn set_zoom(&self, zoom: f64) {
			self.zoom.set(zoom);
			/* We can't use `get_action` here, because the zoom may be set before actions are initialized */
			if let Some(scale_mode) = self.actions.lookup_action("sizing-mode") {
				scale_mode.activate(Some(&"manual".into()));
			}
			self.obj().notify("zoom");

			if let Some(state) = self.state.borrow_mut().as_mut() {
				self.update_layout(state);
			}
			self.obj().queue_draw();
		}

		fn scale_mode_changed(&self) {
			if let Some(state) = self.state.borrow_mut().as_mut() {
				self.update_layout(state);
				self.grab_focus();
			}
		}

		/* Widget size or scale mode changed */
		fn update_layout(&self, state: &mut SongState) {
			let obj = self.obj();
			let zoom = state.change_size(
				obj.width() as f64,
				obj.height() as f64,
				obj.get_scale_mode(),
			);

			{
				self.zoom.set(zoom);
				/* It's rude to trigger potential layout changes during snapshot */
				let obj = obj.clone();
				glib::spawn_future_local(async move {
					obj.notify("zoom");
				});
			}

			/* Cancel the scrolling animation, as the potentially different number of pages invalidates render_offset */
			self.scroll_animation.get().unwrap().skip();
			obj.queue_draw();
		}

		/// One zoom in increment
		fn zoom_in(&self) {
			self.zoom.set((self.zoom.get() / 0.95).clamp(0.6, 3.0));
			self.get_action("sizing-mode")
				.set_state(&"manual".to_variant());
			if let Some(state) = self.state.borrow_mut().as_mut() {
				self.update_layout(state);
			}
			self.obj().grab_focus();
		}

		/// One zoom out increment
		fn zoom_out(&self) {
			self.zoom.set((self.zoom.get() * 0.95).clamp(0.6, 3.0));
			self.get_action("sizing-mode")
				.set_state(&"manual".to_variant());
			if let Some(state) = self.state.borrow_mut().as_mut() {
				self.update_layout(state);
			}
			self.obj().grab_focus();
		}

		/// Set zoom back to 100%
		fn zoom_reset(&self) {
			self.zoom.set(1.0);
			self.get_action("sizing-mode")
				.set_state(&"manual".to_variant());
			if let Some(state) = self.state.borrow_mut().as_mut() {
				self.update_layout(state);
			}
			self.obj().grab_focus();
		}

		/* Events from the zoom gesture */
		#[template_callback]
		fn zoom_gesture_start(&self) {
			log::debug!("Zoom begin");
			self.zoom_before_gesture.set(Some(self.zoom.get()));
		}

		#[template_callback]
		fn zoom_gesture_end(&self) {
			log::debug!("Zoom end");
			self.zoom_before_gesture.set(None);
		}

		#[template_callback]
		fn zoom_gesture_cancel(&self) {
			log::debug!("Zoom cancel");
			self.obj().set_zoom(
				self.zoom_before_gesture
					.take()
					.expect("Should always be Some within after gesture started"),
			);
		}

		#[template_callback]
		fn zoom_gesture_update(&self, scale: f64) {
			self.obj().set_zoom(
				(scale
					* self
						.zoom_before_gesture
						.get()
						.expect("Should always be Some within after gesture started"))
				.clamp(0.6, 3.0),
			);
		}

		#[template_callback]
		fn on_key(&self, keyval: gdk::Key) -> glib::Propagation {
			if keyval == gdk::Key::Left
				|| keyval == gdk::Key::KP_Left
				|| keyval == gdk::Key::Up
				|| keyval == gdk::Key::KP_Up
			{
				self.obj().previous_page();
				glib::Propagation::Stop
			} else if keyval == gdk::Key::Right
				|| keyval == gdk::Key::KP_Right
				|| keyval == gdk::Key::Down
				|| keyval == gdk::Key::KP_Down
			{
				self.obj().next_page();
				glib::Propagation::Stop
			} else {
				glib::Propagation::Proceed
			}
		}

		#[template_callback]
		fn stop_cursor_timer(&self) {
			self.obj().set_cursor(None);
			if let Some(hide_cursor) = self.hide_cursor.borrow_mut().take() {
				hide_cursor.remove();
			}
		}

		#[template_callback]
		pub(super) fn restart_cursor_timer(&self) {
			self.stop_cursor_timer();
			let obj = self.obj().clone();
			*self.hide_cursor.borrow_mut() = Some(glib::source::timeout_add_local_once(
				std::time::Duration::from_secs(4),
				move || {
					obj.imp().hide_cursor.borrow_mut().take();
					obj.set_cursor_from_name(Some("none"));
				},
			));
		}

		/* Focus on click */
		#[template_callback]
		fn on_button_press(&self, _n_press: i32, _x: f64, _y: f64) {
			self.obj().grab_focus();
		}

		#[template_callback]
		fn on_button_release(&self, _n_press: i32, x: f64, _y: f64) {
			let x = x / self.obj().width() as f64;
			if (0.0..0.3).contains(&x) {
				self.obj().previous_page();
			} else if (0.7..1.0).contains(&x) {
				self.obj().next_page();
			}
		}

		/* Scroll events on the page, for zooming */
		#[template_callback]
		fn on_scroll(&self, _dx: f64, dy: f64) -> glib::Propagation {
			if self
				.scroll_gesture
				.current_event_state()
				.contains(gdk::ModifierType::CONTROL_MASK)
			{
				self.obj().set_zoom(
					(if dy > 0.0 {
						self.obj().zoom() * 0.95
					} else {
						self.obj().zoom() / 0.95
					})
					.clamp(0.6, 3.0),
				);
				glib::Propagation::Stop
			} else {
				glib::Propagation::Proceed
			}
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
		}
	}

	/* Returns the calculated zoom. */
	fn change_size(&mut self, width: f64, height: f64, scale_mode: ScaleMode) -> f64 {
		// self.layout = Arc::new(layout::layout_fixed_width(&self.song, width, height, zoom, 10.0));
		// self.layout = Arc::new(layout::layout_fixed_height(&self.song, width, height));
		let zoom = match scale_mode {
			ScaleMode::Zoom(zoom) => zoom,
			ScaleMode::FitStaves(num) => {
				layout::find_scale_for_fixed_staves(&self.song, width, height, num)
			},
			ScaleMode::FitPages(num) => {
				layout::find_scale_for_fixed_columns(&self.song, width, height, num)
			},
		};

		self.layout = layout::layout_fixed_scale(&self.song, width, height, zoom);

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

		zoom
	}

	/* On which staff does our current page start? */
	fn page_start(&self) -> collection::StaffIndex {
		self.layout.get_page_of_staff(self.position).1[0].index
	}

	fn page_end(&self) -> collection::StaffIndex {
		self.layout
			.get_page_of_staff(self.position)
			.1
			.last()
			.unwrap()
			.index
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition?
	 * Returns `None` if there are no repetitions and we should simply go back one page instead.
	 */
	fn section_start(&self) -> Option<collection::StaffIndex> {
		/* Find all sections that are repetitions and are visible on the current page.
		 * Go back to the beginning of the first of them.
		 */
		let page_start = self.page_start();
		self.song
			.section_starts
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

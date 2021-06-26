use dinoscore::library::ScaleMode;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use libhandy::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use typed_index_collections::TiVec;
/* Weird that this is required for it to work */
use super::*;
use actix::Actor;
use dinoscore::*;
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;

pub fn create(
	builder: &woab::BuilderConnector,
	application: gtk::Application,
	library_actor: actix::Addr<library_actor::LibraryActor>,
) -> actix::Addr<SongActor> {
	SongActor::create(move |_ctx| {
		SongActor::new(builder.widgets().unwrap(), application, library_actor)
	})
}

struct SongRenderer {
	pages: TiVec<collection::PageIndex, PageImageBox>,
	song: Arc<collection::SongMeta>,
	image_cache: lru_disk_cache::LruDiskCache,
}

impl SongRenderer {
	fn spawn(
		song: Arc<collection::SongMeta>,
		pages: TiVec<collection::PageIndex, PageImageBox>,
		actor: actix::Addr<SongActor>,
	) -> Sender<UpdateLayout> {
		// TODO move that somewhere else
		let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
		let image_cache = lru_disk_cache::LruDiskCache::new(
			xdg.place_cache_file("staves_small.cache")
				.expect("Could not create cache file"),
			100 * 1024 * 1024,
		)
		.unwrap();

		let (tx, rx) = channel();
		let pages = unsafe { unsafe_force::Send::new(pages) };
		std::thread::spawn(|| {
			SongRenderer {
				pages: unsafe { pages.unwrap() },
				song,
				image_cache,
			}
			.run(rx, actor);
		});
		tx
	}

	fn run(mut self, rx: Receiver<UpdateLayout>, actor: actix::Addr<SongActor>) {
		fn fetch_latest(rx: &Receiver<UpdateLayout>) -> Option<UpdateLayout> {
			rx.try_iter().last()
		}

		let mut update = match rx.recv() {
			Ok(update) => update,
			Err(_) => return,
		};

		'outer: loop {
			/* Cannot use iterators here because of borrow checking */
			for (index, page) in update.layout.pages.iter_enumerated().map(|(index, page)| {
				/* Do the currently visible page first, by swapping the indices */
				if *index == 0 {
					(
						update.current_page,
						&update.layout.pages[update.current_page],
					)
				} else if index == update.current_page {
					(0.into(), &update.layout.pages[layout::PageIndex(0)])
				} else {
					(index, page)
				}
			}) {
				/* Short circuit if there is newer data to be processed */
				if let Some(new_update) = fetch_latest(&rx) {
					update = new_update;
					continue 'outer;
				}
				let surface = self.render_page(page, update.width, update.height);
				actor
					.try_send(UpdatePage {
						index,
						surface: unsafe { unsafe_force::Send::new(surface) },
						layout_id: update.layout.random_id,
					})
					.unwrap();
			}

			/* Next update */
			update = match rx.recv() {
				Ok(update) => update,
				Err(_) => return,
			};
		}
	}

	fn render_staff(&mut self, staff: collection::StaffIndex, width: f64) -> cairo::ImageSurface {
		/*
		 * MIP mapping. For the given width, calculate the actual width we want this to be rendered.
		 * We will never scale an image up and never scale it down more than 2/3
		 */
		let rendered_width = (1.5f64).powf(width.log(1.5).ceil()).ceil();
		// log::debug!("Input width {}, output width {}", width, rendered_width);
		let cache_key = {
			let mut key = std::ffi::OsString::new();
			key.push(self.song.version_uuid.to_string());
			key.push("-");
			key.push((*staff).to_string());
			key.push("-");
			key.push((rendered_width as i32).to_string());
			key
		};
		if self.image_cache.contains_key(&cache_key) {
			// log::debug!("Cache hit for {}", cache_key.to_string_lossy());
			let mut read = self.image_cache.get(&cache_key).unwrap();
			cairo::ImageSurface::create_from_png(&mut read).unwrap_or_else(|e| {
				log::warn!("That cache image seems to be corrupt {}", e);
				/* Remove the corrupt entry and try again */
				self.image_cache.remove(&cache_key).unwrap();
				self.render_staff(staff, width)
			})
		} else {
			// log::warn!("Cache miss for {}", cache_key.to_string_lossy());
			let staff = &self.song.staves[staff];
			let page = &self.pages[staff.page];

			let line_width = staff.width();
			let _line_height = staff.height();
			let aspect_ratio = staff.aspect_ratio();

			let surface = cairo::ImageSurface::create(
				cairo::Format::Rgb24,
				rendered_width as i32,
				(rendered_width * aspect_ratio) as i32,
			)
			.unwrap();
			let context = cairo::Context::new(&surface);

			let scale = surface.get_width() as f64 / line_width;
			context.scale(scale, scale);
			context.translate(-staff.start.0, -staff.start.1);
			context.set_source_rgb(1.0, 1.0, 1.0);
			context.paint();
			page.render(&context);

			surface.flush();
			self.image_cache
				.insert_with(&cache_key, |mut file| {
					surface.write_to_png(&mut file).unwrap_or_else(|e| {
						log::warn!("Could not write image to cache, {}", e);
					});
					Ok(())
				})
				.unwrap();

			surface
		}
	}

	fn render_page(
		&mut self,
		page: &[layout::StaffLayout],
		width: i32,
		height: i32,
	) -> cairo::ImageSurface {
		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width, height).unwrap();
		let context = cairo::Context::new(&surface);
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint();

		page.iter().for_each(|staff_layout| {
			let img = self.render_staff(staff_layout.index, staff_layout.width);
			let scale = staff_layout.width / img.get_width() as f64;

			context.save();
			context.translate(staff_layout.x, staff_layout.y);

			/* Staff */
			context.save();
			context.scale(scale, scale);
			context.set_source_surface(&img, 0.0, 0.0);
			context.paint();
			context.restore();

			/* Page/Staff number */
			context.save();
			context.set_font_size(20.0);
			context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
			context.move_to(10.0, 16.0);
			let (page_index, staff_index) = self.song.page_of_piece(staff_layout.index);
			context.show_text(&format!("{}/{}", *page_index + 1, *staff_index));
			context.restore();

			context.restore();
		});

		surface.flush();
		surface
	}
}

struct UpdateLayout {
	layout: Arc<layout::PageLayout>,
	current_page: layout::PageIndex,
	width: i32,
	height: i32,
}

#[derive(Debug)]
struct SongState {
	song: Arc<collection::SongMeta>,
	page: layout::PageIndex,
	layout: Arc<layout::PageLayout>,
	renderer: Sender<UpdateLayout>,
	zoom: f64,
	scale_mode: ScaleMode,
	/* Backup for when a gesture starts */
	zoom_before_gesture: Option<f64>,
	pdf_page_width: f64,
	/* For each explicit page turn, track the visible staves. Use that to
	 * synchronize the view on layout changes
	 */
	current_staves: Vec<collection::StaffIndex>,
	/* GTK stuff */
	inhibit_cookie: u32,
}

impl SongState {
	fn new(
		renderer: Sender<UpdateLayout>,
		song: Arc<collection::SongMeta>,
		width: f64,
		height: f64,
		pdf_page_width: f64,
		scale_mode: ScaleMode,
		inhibit_cookie: u32,
	) -> Self {
		// let layout = Arc::new(layout::layout_fixed_width(&song, width, height, 1.0, 10.0));
		// let layout = Arc::new(layout::layout_fixed_height(&song, width, height));
		let layout = Arc::new(layout::layout_fixed_scale(
			&song,
			width,
			height,
			1.0,
			pdf_page_width,
		));
		Self {
			song,
			page: 0.into(),
			current_staves: layout.get_staves_of_page(0.into()).collect(),
			layout,
			renderer,
			zoom: 1.0,
			scale_mode,
			zoom_before_gesture: None,
			pdf_page_width,
			inhibit_cookie,
		}
	}

	fn change_size(&mut self, width: f64, height: f64) {
		let layout_staff = self.layout.get_center_staff(self.page);
		// self.layout = Arc::new(layout::layout_fixed_width(&self.song, width, height, zoom, 10.0));
		// self.layout = Arc::new(layout::layout_fixed_height(&self.song, width, height));

		match self.scale_mode {
			ScaleMode::Zoom(_) => {},
			ScaleMode::FitStaves(num) => {
				self.zoom = layout::find_scale_for_fixed_staves(
					&self.song,
					width,
					height,
					num,
					self.pdf_page_width,
				)
			},
			ScaleMode::FitPages(num) => {
				self.zoom = layout::find_scale_for_fixed_columns(
					&self.song,
					width,
					height,
					num,
					self.pdf_page_width,
				)
			},
		}

		dbg!(self.zoom);
		self.layout = Arc::new(layout::layout_fixed_scale(
			&self.song,
			width,
			height,
			self.zoom,
			self.pdf_page_width,
		));
		self.page = self.layout.get_page_of_staff(layout_staff);
	}

	fn get_parts(&self) -> Vec<(collection::StaffIndex, String)> {
		self.song
			.piece_starts
			.iter()
			.map(|(k, v)| {
				(
					*k,
					v.is_empty()
						.then(|| format!("({})", k))
						.unwrap_or_else(|| v.clone()),
				)
			})
			.collect()
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition? */
	fn go_back(&self, current_page: layout::PageIndex) -> Option<layout::PageIndex> {
		/* Find all sections that are repetitions and are visible on the current page.
		 * Go back to the beginning of the first of them.
		 */
		self.song
			.sections()
			.iter()
			.filter(|(_, repetition)| *repetition)
			.map(|(range, _)| range)
			/* Find a section that ends on the current page but starts somewhere before */
			.find(|range| {
				self.layout.get_page_of_staff(*range.end()) == current_page
					&& self.layout.get_page_of_staff(*range.start()) < current_page
			})
			.map(|range| self.layout.get_page_of_staff(*range.start()))
	}

	/* When we're at a given position, where did the part we are in start? */
	fn part_start(&self, current_page: layout::PageIndex) -> collection::StaffIndex {
		self.song
			.piece_starts
			.iter()
			.filter_map(|(part, _)| {
				if self.layout.get_page_of_staff(*part) <= current_page {
					Some(*part)
				} else {
					None
				}
			})
			.max()
			.unwrap_or_else(|| 0.into())
	}
}

pub struct SongActor {
	widgets: SongWidgets,
	application: gtk::Application,
	actions: gio::SimpleActionGroup,
	/// Always Some once started. Could have used a lazy init cell as well
	part_selection_changed_signal: Option<glib::SignalHandlerId>,
	song: Option<SongState>,
	next: gio::SimpleAction,
	previous: gio::SimpleAction,
	next_piece: gio::SimpleAction,
	previous_piece: gio::SimpleAction,
	zoom_gesture: gtk::GestureZoom,
	sizing_mode_action: gio::SimpleAction,
	last_interaction: std::time::Instant,
	library_actor: actix::Addr<LibraryActor>,
	annotations: Rc<RefCell<Option<poppler::PopplerDocument>>>,
}

#[derive(woab::WidgetsFromBuilder)]
pub struct SongWidgets {
	header: libhandy::HeaderBar,
	carousel: libhandy::Carousel,
	carousel_box: gtk::Box,
	deck: libhandy::Deck,
	part_selection: gtk::ComboBoxText,
	zoom_button: gtk::MenuButton,
	zoom_menu: gio::Menu,
}

impl actix::Actor for SongActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		self.widgets
			.carousel_box
			.insert_action_group("view_actions", Some(&self.actions));

		self.actions.add_action(&self.next);
		self.application
			.set_accels_for_action("view_actions.next_page", &["<Primary>N", "<Alt>N", "Right"]);
		woab::route_action(&self.next, ctx.address()).unwrap();

		self.actions.add_action(&self.previous);
		self.application.set_accels_for_action(
			"view_actions.previous_page",
			&["<Primary>P", "<Alt>P", "Left"],
		);
		woab::route_action(&self.previous, ctx.address()).unwrap();

		let go_back_action = gio::SimpleAction::new("go_back", None);
		self.actions.add_action(&go_back_action);
		self.application
			.set_accels_for_action("view_actions.go_back", &["Escape"]);
		woab::route_action(&go_back_action, ctx.address()).unwrap();

		self.actions.add_action(&self.previous_piece);
		self.application
			.set_accels_for_action("view_actions.previous_piece", &[]);
		woab::route_action(&self.previous_piece, ctx.address()).unwrap();

		self.actions.add_action(&self.next_piece);
		self.application
			.set_accels_for_action("view_actions.next_piece", &[]);
		woab::route_action(&self.next_piece, ctx.address()).unwrap();

		self.part_selection_changed_signal = Some(
			woab::route_signal(
				&self.widgets.part_selection,
				"changed",
				"SelectPart",
				ctx.address(),
			)
			.unwrap(),
		);

		let zoom_gesture = &self.zoom_gesture;
		woab::route_signal(zoom_gesture, "begin", "ZoomBegin", ctx.address()).unwrap();
		woab::route_signal(zoom_gesture, "end", "ZoomEnd", ctx.address()).unwrap();
		woab::route_signal(zoom_gesture, "cancel", "ZoomCancel", ctx.address()).unwrap();
		woab::route_signal(
			zoom_gesture,
			"scale-changed",
			"ZoomScaleChanged",
			ctx.address(),
		)
		.unwrap();

		let zoom_popover = gtk::Popover::from_model(None::<&gtk::Widget>, &self.widgets.zoom_menu);
		self.widgets.zoom_button.set_popover(Some(&zoom_popover));

		self.actions.add_action(&self.sizing_mode_action);
		woab::route_signal(
			&self.sizing_mode_action,
			"activate",
			"ScaleModeChanged",
			ctx.address(),
		)
		.unwrap();

		let zoom_in = gio::SimpleAction::new("zoom-in", None);
		self.actions.add_action(&zoom_in);
		self.application
			.set_accels_for_action("view_actions.zoom-in", &["<Primary>plus"]);
		woab::route_signal(&zoom_in, "activate", "ZoomIn", ctx.address()).unwrap();

		let zoom_out = gio::SimpleAction::new("zoom-out", None);
		self.actions.add_action(&zoom_out);
		self.application
			.set_accels_for_action("view_actions.zoom-out", &["<Primary>minus"]);
		woab::route_signal(&zoom_out, "activate", "ZoomOut", ctx.address()).unwrap();

		let zoom_original = gio::SimpleAction::new("zoom-original", None);
		self.actions.add_action(&zoom_original);
		self.application
			.set_accels_for_action("view_actions.zoom-original", &["<Primary>0"]);
		woab::route_signal(&zoom_original, "activate", "ZoomOriginal", ctx.address()).unwrap();

		glib::timeout_add_seconds_local(1, || Continue(true));

		/* MIDI handling */
		let (midi_tx, midi_rx) =
			glib::MainContext::channel::<pedal::PageEvent>(glib::Priority::default());
		let handler = pedal::run(midi_tx).unwrap();
		use actix::AsyncContext;
		let address = ctx.address();
		midi_rx.attach(None, move |event| {
			/* Reference the MIDI handler which holds the Sender so that it doesn't get dropped. */
			let _handler = &handler;
			address.try_send(event).unwrap();
			Continue(true)
		});
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("SongActor Quit");
	}
}

impl SongActor {
	pub fn new(
		widgets: SongWidgets,
		application: gtk::Application,
		library_actor: actix::Addr<LibraryActor>,
	) -> Self {
		let next = gio::SimpleAction::new("next_page", None);
		let previous = gio::SimpleAction::new("previous_page", None);
		let next_piece = gio::SimpleAction::new("next_piece", None);
		let previous_piece = gio::SimpleAction::new("previous_piece", None);
		Self {
			zoom_gesture: gtk::GestureZoom::new(&widgets.carousel),
			widgets,
			application,
			actions: gio::SimpleActionGroup::new(),
			song: None,
			next,
			previous,
			next_piece,
			previous_piece,
			part_selection_changed_signal: None,
			sizing_mode_action: gio::SimpleAction::new_stateful(
				"sizing-mode",
				Some(&String::static_variant_type()),
				&"manual".to_variant(),
			),
			last_interaction: std::time::Instant::now(),
			library_actor,
			annotations: Rc::new(RefCell::new(None)),
		}
	}

	fn load_song(
		&mut self,
		ctx: &mut actix::Context<Self>,
		song: collection::SongMeta,
		pages: TiVec<collection::PageIndex, PageImageBox>,
		scale_mode: ScaleMode,
	) {
		let inhibit_cookie = self.application.inhibit(
			Option::<&gtk::Window>::None,
			gtk::ApplicationInhibitFlags::IDLE,
			Some("You wouldn't want your screen go blank while playing an instrument"),
		);

		use actix::AsyncContext;

		let song = Arc::new(song);
		// TODO make this per page, and less ugly please
		let pdf_page_width = pages[collection::PageIndex(0)].get_width();
		let renderer = SongRenderer::spawn(song.clone(), pages, ctx.address());
		let width = self.widgets.carousel.get_allocated_width();
		let height = self.widgets.carousel.get_allocated_height();

		self.widgets.header.set_title(
			song.title
				.as_ref()
				.map(|title| format!("{} – DiNoScore", title))
				.as_deref()
				.or(Some("DiNoScore")),
		);
		self.widgets.carousel_box.grab_focus();

		let song = SongState::new(
			renderer,
			song,
			width as f64,
			height as f64,
			pdf_page_width,
			scale_mode,
			inhibit_cookie,
		);

		let parts = song.get_parts();
		self.widgets.part_selection.remove_all();
		for (k, p) in &parts {
			self.widgets.part_selection.append(Some(&k.to_string()), p);
		}
		let relevant = parts.len() > 1;
		woab::spawn_outside(clone!(
			@weak self.widgets.part_selection as part_selection,
			@weak self.sizing_mode_action as sizing_mode_action
		=> @default-panic, move || {async move{
			part_selection.set_active(if relevant {Some(0)} else {None});
			part_selection.set_sensitive(relevant);

			sizing_mode_action.set_state(&scale_mode.action_string().to_variant());
		}})());

		self.song = Some(song);
		self.update_content(ctx);
		self.load_annotations();
	}

	fn update_manual_zoom(
		&mut self,
		modify_zoom: impl FnOnce(&mut SongState) -> f64,
		ctx: &mut actix::Context<Self>,
	) {
		if let Some(song) = self.song.as_mut() {
			self.sizing_mode_action.set_state(&"manual".to_variant());
			song.zoom = modify_zoom(song);
			song.scale_mode = ScaleMode::Zoom(song.zoom as f32);
			self.update_content(ctx);
			self.update_timer();
		}
	}

	fn update_content(&mut self, ctx: &mut actix::Context<Self>) {
		let width = self.widgets.carousel.get_allocated_width();
		let height = self.widgets.carousel.get_allocated_height();
		if width == 1 || height == 1 {
			return;
		}

		let song = match &mut self.song {
			Some(song) => song,
			None => return,
		};

		song.change_size(width as f64, height as f64);
		self.widgets
			.zoom_button
			.set_label(&format!("{:.0}%", song.zoom * 100.0));

		let carousel = &self.widgets.carousel;
		let new_pages = song.layout.pages.len();
		let old_pages = carousel.get_n_pages() as usize;
		use std::cmp::Ordering;
		match new_pages.cmp(&old_pages) {
			Ordering::Equal => { /* Be happy and do nothing */ },
			Ordering::Greater => {
				/* Add missing pages */
				for _ in old_pages..new_pages {
					let area = gtk::DrawingArea::new();
					use actix::AsyncContext;
					area.add_events(gdk::EventMask::SCROLL_MASK);
					woab::route_signal(&area, "scroll-event", "AreaScroll", ctx.address()).unwrap();

					woab::spawn_outside(
						clone!(@weak carousel => @default-panic, move || {async move {
							area.set_hexpand(true);
							area.set_vexpand(true);

							// area.connect_draw(move |_area, context| {
							// 	context.set_source_rgb(1.0, 0.0, 1.0);
							// 	context.paint();
							// 	gtk::Inhibit::default()
							// });

							carousel.add(&area);
							area.show();
						}})(),
					);
				}
			},
			Ordering::Less => {
				/* Remove excess pages */
				woab::spawn_outside(
					clone!(@weak carousel => @default-panic, move || {async move {
						for page in &carousel.get_children()[new_pages..old_pages] {
							carousel.remove(page);
						}
					}})(),
				);
			},
		}

		woab::spawn_outside(
			clone!(@weak carousel => @default-panic, move || {async move {
				carousel.queue_draw();
			}})(),
		);
		/* Calculate the new page, which has the most staves in common with the previous layout/page */
		let new_page: layout::PageIndex = {
			use itertools::Itertools;

			song.current_staves
				.iter()
				.copied()
				.map(|staff| song.layout.get_page_of_staff(staff))
				.counts()
				.iter()
				.max_by(|(a_page, a_count), (b_page, b_count)| {
					/* We want smallest page with the most number of hits */
					a_count
						.cmp(b_count)
						.then_with(|| a_page.cmp(b_page).reverse())
				})
				.map(|(page, _count)| *page)
				.unwrap()
		};
		woab::spawn_outside(
			clone!(@weak carousel => @default-panic, move || {async move {
				carousel.scroll_to_full(&carousel.get_children()[*new_page], 0);
			}})(),
		);

		song.page = new_page;

		song.renderer
			.send(UpdateLayout {
				layout: song.layout.clone(),
				current_page: song.page,
				width,
				height,
			})
			.unwrap();
	}

	fn update_timer(&mut self) {
		let last_interaction = std::time::Instant::now();
		let diff = last_interaction
			.duration_since(self.last_interaction)
			.as_secs()
			/* Consider everything about 3 minutes as "idle" */
			.min(180);
		self.library_actor
			.try_send(library_actor::UpdateSongUsage {
				seconds_elapsed: diff,
				song: self.song.as_ref().unwrap().song.song_uuid,
				scale_mode: self.song.as_ref().unwrap().scale_mode,
			})
			.unwrap();
		self.last_interaction = last_interaction;
	}

	fn load_annotations(&mut self) {
		if let Some(song) = &self.song {
			log::debug!("Reloading annotations");
			let uuid = song.song.song_uuid;
			// TODO don't hardcode here
			let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
			let annotations_export = xdg
				.place_data_file(format!("annotations/{}.pdf", uuid))
				.unwrap();

			let document = annotations_export
				.exists()
				.then(|| poppler::PopplerDocument::new_from_file(annotations_export, "").unwrap());
			*self.annotations.borrow_mut() = document;
			let carousel = self.widgets.carousel.clone();
			woab::spawn_outside(async move {
				carousel.queue_draw();
				carousel.foreach(WidgetExt::queue_draw);
			});
		}
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct UpdatePage {
	index: layout::PageIndex,
	surface: unsafe_force::Send<cairo::ImageSurface>,
	/// Unique random identifier to ignore old data
	layout_id: uuid::Uuid,
}

impl actix::Handler<UpdatePage> for SongActor {
	type Result = ();

	fn handle(&mut self, page: UpdatePage, _ctx: &mut Self::Context) -> Self::Result {
		if let Some(song) = self.song.as_ref() {
			if page.layout_id != song.layout.random_id {
				return;
			}
			// log::debug!("Updating page {}", page.index);
			let area = &self.widgets.carousel.get_children()[*page.index];
			let area: &gtk::DrawingArea = area.downcast_ref().unwrap();
			let surface = unsafe { page.surface.unwrap() };
			let page = page.index;
			let layout = song.layout.pages[page].clone();
			let song = song.song.clone();
			let annotations = self.annotations.clone();
			area.connect_draw(move |area, context| {
				context.set_source_rgb(1.0, 1.0, 1.0);
				context.paint();
				if surface.get_width() != area.get_allocated_width()
					|| surface.get_height() != area.get_allocated_height()
				{
					/* Scaling is simply too slow */
					// context.scale(
					// 	area.get_allocated_width() as f64 / surface.get_width() as f64,
					// 	area.get_allocated_height() as f64 / surface.get_height() as f64,
					// );
					context.set_source_surface(
						&surface,
						(area.get_allocated_width() - surface.get_width()) as f64 / 2.0,
						(area.get_allocated_height() - surface.get_height()) as f64 / 2.0,
					);
				} else {
					context.set_source_surface(&surface, 0.0, 0.0);
				}
				context.paint();

				if let Some(document) = annotations.borrow().as_ref() {
					/* render annotations */
					for staff_layout in &layout {
						let staff = &song.staves[staff_layout.index];
						let page = document.get_page(*staff.page).unwrap();

						context.save();
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

						page.render(&context);

						context.restore();
					}
				}
				gtk::Inhibit::default()
			});
			area.queue_draw();
			self.widgets.carousel.queue_draw();
		}
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct LoadSong {
	pub meta: collection::SongMeta,
	pub pages: unsafe_force::Send<TiVec<collection::PageIndex, PageImageBox>>,
	pub scale_mode: ScaleMode,
}

impl actix::Handler<LoadSong> for SongActor {
	type Result = ();

	fn handle(&mut self, song: LoadSong, ctx: &mut Self::Context) -> Self::Result {
		self.load_song(
			ctx,
			song.meta,
			unsafe { song.pages.unwrap() },
			song.scale_mode,
		);
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct UpdateAnnotations;

impl actix::Handler<UpdateAnnotations> for SongActor {
	type Result = ();

	fn handle(&mut self, _: UpdateAnnotations, _ctx: &mut Self::Context) -> Self::Result {
		self.load_annotations();
	}
}

impl actix::Handler<pedal::PageEvent> for SongActor {
	type Result = ();

	fn handle(&mut self, event: pedal::PageEvent, _ctx: &mut Self::Context) -> Self::Result {
		match event {
			pedal::PageEvent::Next => {
				self.next.activate(None);
			},
			pedal::PageEvent::Previous => {
				self.previous.activate(None);
			},
		}
	}
}

impl actix::Handler<woab::Signal> for SongActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, ctx: &mut Self::Context) -> woab::SignalResult {
		if self.song.as_ref().is_some() {
			self.update_timer();
		}

		let carousel = &self.widgets.carousel;
		signal!(match (signal) {
			/* Switch pages */
			"next_page" => {
				if self.song.is_some() {
					let new_page = usize::min(carousel.get_position() as usize + 1, carousel.get_n_pages() as usize - 1);
					carousel.scroll_to(&carousel.get_children()[new_page]);
				}
			},
			"previous_page" => {
				if let Some(song) = self.song.as_ref() {
					let new_page = song.go_back(layout::PageIndex(carousel.get_position() as usize))
						.unwrap_or_else(|| layout::PageIndex(usize::max(carousel.get_position() as usize, 1) - 1));
					carousel.scroll_to(&carousel.get_children()[*new_page]);
				}
			},
			"previous_piece" => {
				if let Some(song) = self.song.as_mut() {
					let staff = song.current_staves[0] - 1.into();
					let (previous_piece_staff, _) = song.song.piece_starts.range(..=staff)
						.next_back()
						.expect("That button should have been disabled");
					let page = *song.layout
						.get_page_of_staff(*previous_piece_staff);

					let carousel = carousel.clone();
					woab::spawn_outside(async move {
						carousel.scroll_to(&carousel.get_children()[page]);
					});
				}
			},
			"next_piece" => {
				if let Some(song) = self.song.as_mut() {
					let staff = song.current_staves.iter().next_back().unwrap();
					let (next_piece_staff, _) = song.song.piece_starts.range(staff..)
						.next()
						.expect("That button should have been disabled");
					let page = *song.layout
						.get_page_of_staff(*next_piece_staff);

					let carousel = carousel.clone();
					woab::spawn_outside(async move {
						carousel.scroll_to(&carousel.get_children()[page]);
					});
				}
			},
			"annotate" => {
				log::debug!("annotate!");
				if let Some(song) = self.song.as_mut() {
					self.library_actor.try_send(library_actor::RunEditor {
						song: song.song.song_uuid,
						page: song.song.staves[song.current_staves[0]].page,
					}).unwrap();
				}
			},
			/* Unload the song */
			"go_back" => {
				let song = self.song.take().unwrap();
				self.application.uninhibit(song.inhibit_cookie);
				std::mem::drop(song);
				self.widgets.carousel.foreach(|p| self.widgets.carousel.remove(p));
				self.annotations.borrow_mut().take();

				woab::spawn_outside(clone!(
					@weak self.widgets.part_selection as part_selection,
					@weak self.widgets.deck as deck
				=> @default-panic, move || {async move {
					part_selection.set_active(None);
					part_selection.set_sensitive(false);
					part_selection.remove_all();

					deck.navigate(libhandy::NavigationDirection::Back);
				}})());
			},
			/* From the dropdown */
			"SelectPart" => {if self.song.is_some() {
				let section = self.widgets.part_selection.get_active_id().unwrap();
				let carousel = carousel.clone();
				let page = *self.song.as_ref().unwrap()
					.layout
					.get_page_of_staff(section.parse::<collection::StaffIndex>().unwrap());
				woab::spawn_outside(async move {
					carousel.scroll_to(&carousel.get_children()[page]);
				});
			}},
			"CarouselSizeChanged" => {self.update_content(ctx)},
			// TODO add cooldown
			// TODO don't trigger on top of a swipe gesture
			"CarouselButtonPress" => |carousel = libhandy::Carousel, event = gdk::Event| {
				let event: gdk::EventButton = event.downcast().unwrap();

				let x = event.get_position().0 / carousel.get_allocated_width() as f64;
				return Ok(Some(gtk::Inhibit((0.0..0.3).contains(&x) || (0.6..1.0).contains(&x))));
			},
			"CarouselButtonRelease" => |carousel = libhandy::Carousel, event = gdk::Event| {
				let event: gdk::EventButton = event.downcast().unwrap();

				let x = event.get_position().0 / carousel.get_allocated_width() as f64;
				if (0.0..0.3).contains(&x) {
					woab::spawn_outside(clone!(@weak self.previous as previous => @default-panic, move || {async move {
						previous.activate(None);
					}})());
				} else if (0.6..1.0).contains(&x) {
					woab::spawn_outside(clone!(@weak self.next as next => @default-panic, move || {async move {
						next.activate(None);
					}})());
				}

				let x = event.get_position().0 / carousel.get_allocated_width() as f64;
				return Ok(Some(gtk::Inhibit((0.0..0.3).contains(&x) || (0.6..1.0).contains(&x))));
			},
			"CarouselPageChanged" => |_, page = u32| {
				if let Some(song) = self.song.as_mut() {
					song.page = layout::PageIndex(page as usize);
					song.current_staves = song.layout.get_staves_of_page(song.page).collect();
					let active_id = song.part_start(song.page).to_string();

					self.previous_piece.set_enabled(page > 0);
					self.next_piece.set_enabled(
						song.song.piece_starts.range(song.current_staves.iter().next_back().unwrap()..)
							.next()
							.is_some()
					);

					/* TODO clean this up a bit */
					use actix::{WrapFuture, ActorFutureExt, AsyncContext};
					let part_selection_changed_signal = self.part_selection_changed_signal.as_ref().unwrap();
					self.widgets.part_selection.block_signal(part_selection_changed_signal);
					let fut = clone!(
						@weak self.widgets.part_selection as part_selection
					=> @default-panic, move || woab::outside(async move {
						part_selection.set_active_id(Some(&active_id));
					}))();
					let fut = fut
					.into_actor(self)
					.map(move |result, this: &mut Self, _ctx| {
						result.unwrap();
						this.widgets.part_selection.unblock_signal(this.part_selection_changed_signal.as_ref().unwrap());
					});
					ctx.spawn(
						fut
					);
				}
			},

			/* Events from the zoom gesture */
			"ZoomBegin" => {
				log::debug!("Begin");
				if let Some(song) = self.song.as_mut() {
					song.zoom_before_gesture = Some(song.zoom);
				}
			},
			"ZoomEnd" => {
				log::debug!("End");
				if let Some(song) = self.song.as_mut() {
					song.zoom_before_gesture = None;
				}
			},
			"ZoomCancel" => {
				log::debug!("Cancel");
				self.update_manual_zoom(|song| {
					song.zoom_before_gesture.take()
						//.expect("Should always be Some within after gesture started");
						.unwrap_or(song.zoom)
				}, ctx);
			},
			"ZoomScaleChanged" => |_ = gtk::GestureZoom, scale = f64| {
				dbg!(scale);
				self.update_manual_zoom(|song| {
					let zoom = scale * song.zoom_before_gesture.expect("Should always be Some within after gesture started");
					zoom.clamp(0.6, 3.0)
				}, ctx);
			},

			/* Scroll events on the page, for zooming */
			"AreaScroll" => |_ = gtk::DrawingArea, event = gdk::Event| {
				let event: gdk::EventScroll = event.downcast().unwrap();
				if event.get_state().contains(gdk::ModifierType::CONTROL_MASK) {
					self.update_manual_zoom(|song| {
						let zoom = song.zoom * (if event.get_direction() == gdk::ScrollDirection::Down {0.95} else {1.0/0.95});
						zoom.clamp(0.6, 3.0)
					}, ctx);
				}
				// return Some(Inhibit(false));
				return Ok(Some(gtk::Inhibit(event.get_state().contains(gdk::ModifierType::CONTROL_MASK))));
			},

			/* Generic zoom events */
			"ZoomIn" => { self.update_manual_zoom(|song| (song.zoom / 0.95).clamp(0.6, 3.0), ctx); },
			"ZoomOut" => { self.update_manual_zoom(|song| (song.zoom * 0.95).clamp(0.6, 3.0), ctx); },
			"ZoomOriginal" => { self.update_manual_zoom(|_| 1.0, ctx); },

			"ScaleModeChanged" => |action = gio::SimpleAction, mode = glib::Variant| {
				action.set_state(&mode);
				if let Some(song) = self.song.as_mut() {
					song.scale_mode = match mode.get::<String>().unwrap().as_str() {
						"fit-staves" => ScaleMode::FitStaves(3),
						"fit-columns" => ScaleMode::FitPages(2),
						"manual" => return Ok(None),
						invalid => unreachable!(format!("Invalid value: '{}'", invalid)),
					};
					self.update_content(ctx);
					self.update_timer();
				}
			},
		});
		Ok(None)
	}
}

extern crate gdk;
extern crate gdk_pixbuf;
extern crate gio;
extern crate glib;
extern crate gtk;

#[macro_use]
extern crate maplit;
extern crate derive_more;

use futures::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
extern crate cairo;

use libhandy::prelude::*;
/* Weird that this is required for it to work */
use libhandy::prelude::HeaderBarExt;

use std::{cell::RefCell, rc::Rc};

extern crate serde_json;

extern crate either;

use noisy_float::prelude::*;

pub const EXPERIMENT_MODE: bool = true;

use dinoscore::{song::*, *};

#[derive(Clone, Debug)]
struct PageLayout {
	/// Pages[Columns]
	pages: Vec<Vec<StaffLayout>>,
}

impl PageLayout {
	fn new(
		song: &Song,
		width: f64,
		height: f64,
		zoom: f64,
		column_count: usize,
		spacing: f64,
	) -> Self {
		if EXPERIMENT_MODE {
			return PageLayout::new_alternate(song, width, height, column_count);
		}
		let column_width = (width / column_count as f64) * zoom;
		/* 1. Segment the staves to fit onto columns */
		let column_starts = {
			let mut column_starts = Vec::<StaffIndex>::new();
			let mut y = 0.0;
			// let mut y_with_spacing = 0.0;
			for (index, staff) in song.staves.iter().enumerate() {
				let index = StaffIndex(index);

				let staff_height = column_width * staff.aspect_ratio;

				/*
				 * Start a new column for a new piece.
				 * If staff doesn't fit anymore, first try to squeeze it in at the cost of spacing
				 */
				if song.piece_starts.contains_key(&index)
					// || ((y_with_spacing + staff_height > height) && (y + staff_height <= height))
					|| (y + staff_height > height)
				{
					y = 0.0;
					// y_with_spacing = 0.0;
					column_starts.push(index);
				}
				y += staff_height;
				// y_with_spacing += staff_height + spacing;
			}
			/* Without this the last page will get swallowed */
			column_starts.push(song.staves.len().into());
			column_starts
		};

		/* 2. Calculate the exact position of each staff */
		let columns: Vec<Vec<StaffLayout>> = column_starts
			.windows(2)
			.map(|v| (v[0], v[1]))
			.map(|(chunk_start, chunk_end)| {
				let mut column = Vec::new();
				let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
				if staves.len() == 1 {
					let staff = &staves[0];
					let staff_height = column_width * staff.aspect_ratio;
					let x;
					let y;
					let staff_width;

					if staff_height > height {
						staff_width = height / staff.aspect_ratio;
						x = (column_width - staff_width) / 2.0;
						y = 0.0;
					} else {
						staff_width = column_width;
						x = 0.0;
						y = (height - staff_height) / 2.0;
					}

					column.push(StaffLayout {
						index: chunk_start,
						x,
						y,
						width: staff_width,
					});
				} else {
					let excess_space = height
						- staves
							.iter()
							.map(|staff| column_width * staff.aspect_ratio)
							.sum::<f64>();
					let spacing = f64::min(spacing, excess_space / staves.len() as f64);
					let mut y = (excess_space - spacing * staves.len() as f64) / 2.0;
					for (index, staff) in staves.iter().enumerate() {
						column.push(StaffLayout {
							index: chunk_start + StaffIndex(index),
							x: 0.0,
							y,
							width: column_width,
						});
						y += column_width * staff.aspect_ratio + spacing;
					}
				}
				column
			})
			.collect();

		/* 3. Merge the single columns to pages using iterator magic */
		let left_margin = (width - width * zoom) / 2.0;
		let pages = columns
			.chunks(column_count)
			.map(|chunk| {
				chunk
					.iter()
					.enumerate()
					.flat_map(|(i, c)| {
						c.iter().map(move |staff| StaffLayout {
							index: staff.index,
							x: staff.x + column_width * (i % column_count) as f64 + left_margin,
							y: staff.y,
							width: staff.width,
						})
					})
					.collect()
			})
			.collect();
		PageLayout { pages }
	}

	fn new_alternate(song: &Song, width: f64, height: f64, row_count: usize) -> Self {
		let row_height = height / row_count as f64;

		let column_starts = {
			let mut column_starts = Vec::<StaffIndex>::new();
			let mut page_length = 0;
			for index in 0..song.staves.len() {
				let index = StaffIndex(index);

				if song.piece_starts.contains_key(&index) || page_length >= row_count {
					column_starts.push(index);
					page_length = 0;
				}
				page_length += 1;
			}
			column_starts.push(song.staves.len().into());
			column_starts
		};

		let pages = column_starts
			.windows(2)
			.map(|v| (v[0], v[1]))
			.map(|(chunk_start, chunk_end)| {
				let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
				let max_width: f64 = staves
					.iter()
					.map(|staff| r64(row_height / staff.aspect_ratio))
					.min() /* min is correct here */
					.expect("Page cannot be empty")
					.into();
				let max_width = max_width.min(width);

				staves
					.iter()
					.enumerate()
					.map(|(in_page_index, staff)| {
						let mut staff_width = row_height / staff.aspect_ratio;
						let mut staff_height = row_height;

						if staff_width > max_width {
							staff_width = max_width;
							staff_height *= staff_width / max_width;
						}

						StaffLayout {
							index: StaffIndex(in_page_index) + chunk_start,
							x: (width - staff_width) / 2.0,
							y: in_page_index as f64 * row_height
								+ (row_height - staff_height) / 2.0,
							width: staff_width,
						}
					})
					.collect::<Vec<_>>()
			})
			.collect::<Vec<_>>();

		PageLayout { pages }
	}

	/** Get the index of the staff at the center of the page. */
	fn get_center_staff(&self, page: PageIndex) -> StaffIndex {
		StaffIndex(
			self.pages[0..*page].iter().map(Vec::len).sum::<usize>() + self.pages[*page].len() / 2,
		)
	}

	fn get_staves_of_page<'a>(&'a self, page: PageIndex) -> impl Iterator<Item = StaffIndex> + 'a {
		self.pages[*page].iter().map(|page| page.index)
	}

	fn get_page_of_staff(&self, staff: StaffIndex) -> PageIndex {
		let mut sum = 0;
		// dbg!(&self.pages, &staff);
		for (i, page) in self.pages.iter().enumerate() {
			sum += page.len();
			if sum > staff.into() {
				return i.into();
			}
		}
		unreachable!()
	}
}

#[derive(Debug)]
struct ViewerState {
	song: Song,
	page: PageIndex,
	layout: PageLayout,
	/* To keep the current view consistent between layout changes */
	columns: usize,
	zoom: f64,
	/* Backup for when a gesture starts */
	zoom_before_gesture: Option<f64>,
}

impl ViewerState {
	fn new(song: Song, columns: usize, width: f64, height: f64) -> Self {
		let layout = PageLayout::new(&song, width, height, 1.0, columns, 10.0);
		ViewerState {
			song,
			page: 0.into(),
			layout,
			columns,
			zoom: 1.0,
			zoom_before_gesture: None,
		}
	}

	fn change_size(&mut self, width: f64, height: f64, columns: usize, zoom: f64) {
		self.columns = columns;
		self.zoom = zoom;
		let layout_staff = self.layout.get_center_staff(self.page);
		self.layout = PageLayout::new(&self.song, width, height, zoom, self.columns, 10.0);
		self.page = self.layout.get_page_of_staff(layout_staff);
	}

	fn get_parts(&self) -> Vec<(StaffIndex, String)> {
		self.song
			.piece_starts
			.iter()
			.map(|(k, v)| (*k, v.clone().unwrap_or_else(|| format!("({})", k))))
			.collect()
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition? */
	fn go_back(&self, current_page: PageIndex) -> Option<PageIndex> {
		/* Find all sections that are repetitions and are visible on the current page.
		 * Go back to the beginning of the first of them.
		 */
		self.song
			.sections
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
	fn part_start(&self, current_page: PageIndex) -> StaffIndex {
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

struct SheetViewer {
	carousel: libhandy::Carousel,
	update_task: ReplaceIdentifier<()>,
	// Keep a strong reference to this
	#[allow(dead_code)]
	zoom_gesture: gtk::GestureZoom,
}

mod unique_future;
use unique_future::*;

impl SheetViewer {
	fn new(carousel: &libhandy::Carousel) -> Self {
		carousel.add_events(
			gdk::EventMask::STRUCTURE_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::KEY_PRESS_MASK,
		);
		carousel.connect_button_press_event(move |carousel, _| {
			carousel.emit_grab_focus();
			gtk::Inhibit(false)
		});
		carousel.set_focus_on_click(true);
		carousel.set_can_focus(true);

		let zoom_gesture = gtk::GestureZoom::new(carousel);

		SheetViewer {
			carousel: carousel.clone(),
			update_task: ReplaceIdentifier::new(),
			zoom_gesture,
		}
	}

	fn update(this: &Rc<RefCell<Self>>, state: &Rc<RefCell<Option<ViewerState>>>) {
		let future = {
			let carousel = &this.borrow().carousel;
			let width = carousel.get_allocated_width();
			let height = carousel.get_allocated_height();
			let carousel = carousel.clone();
			let state = state.clone();
			let this = this.clone();
			async move {
				let new_images = SheetViewer::update_state(width, height, &*state.borrow()).await;
				SheetViewer::update_sheet(&this, &carousel, new_images, &state).await;
			}
		};
		let future = this
			.borrow_mut()
			.update_task
			.make_replaceable(future)
			.map(|_| {});

		glib::MainContext::default()
			.spawn_local_with_priority(glib::source::PRIORITY_DEFAULT_IDLE, future);
	}

	// The foreground operation
	async fn update_sheet(
		this: &Rc<RefCell<Self>>,
		carousel: &libhandy::Carousel,
		pages: Vec<cairo::ImageSurface>,
		state: &Rc<RefCell<Option<ViewerState>>>,
	) {
		carousel.foreach(|p| carousel.remove(p));
		futures::stream::iter(pages.into_iter()).enumerate().for_each(|(_index, page)| async move {
			let area = gtk::DrawingArea::new();
			area.set_hexpand(true);
			area.set_vexpand(true);
			area.connect_draw(move |_area, context| {
				context.set_source_surface(&page, 0.0, 0.0);
				context.paint();
				gtk::Inhibit::default()
			});
			area.add_events(gdk::EventMask::SCROLL_MASK);
			area.connect_scroll_event(clone!(@weak carousel, @strong state, @strong this => @default-panic, move |_area, event| {
				if event.get_state().contains(gdk::ModifierType::CONTROL_MASK) {
					if let Some(state) = &mut *state.borrow_mut() {
						let new_zoom = state.zoom * (if event.get_direction() == gdk::ScrollDirection::Down {0.95} else {1.0/0.95});
						// TODO replace with clamp once stable
						let new_zoom = f64::max(0.2, f64::min(1.0, new_zoom));
						state.change_size(carousel.get_allocated_width() as f64, 
							carousel.get_allocated_height() as f64, state.columns, new_zoom);
					}
					SheetViewer::update(&this, &state);
					gtk::Inhibit(true)
				} else {
					gtk::Inhibit(false)
				}
			}));

			carousel.add(&area);
			area.show();
		}).await;
		carousel.queue_draw();
		if let Some(state) = &*state.borrow() {
			carousel.scroll_to_full(&carousel.get_children()[*state.page], 0);
		}
	}

	// The background operation
	async fn update_state(
		width: i32,
		height: i32,
		state: &Option<ViewerState>,
	) -> Vec<cairo::ImageSurface> {
		if let Some(state) = state {
			let song = &state.song;
			futures::stream::iter(state.layout.pages.iter())
				.then(|page| async move {
					let surface =
						cairo::ImageSurface::create(cairo::Format::Rgb24, width, height).unwrap();
					let context = cairo::Context::new(&surface);
					context.set_source_rgb(1.0, 1.0, 1.0);
					context.paint();

					futures::stream::iter(page.iter())
						.for_each(|staff_layout| {
							let context = context.clone();
							async move {
								song.staves[*staff_layout.index].render(&context, &staff_layout);
							}
						})
						.await;

					surface.flush();
					surface
				})
				.collect()
				.await
		} else {
			Vec::new()
		}
	}
}

fn build_ui(application: &gtk::Application) {
	application.inhibit(
		Option::<&gtk::Window>::None,
		gtk::ApplicationInhibitFlags::IDLE,
		Some("You wouldn't want your screen go blank while playing an instrument"),
	);

	/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
	let _ = gio::ThemedIcon::static_type();

	let builder = gtk::Builder::from_file("res/viewer.glade");
	let window: gtk::Window = builder.get_object("window").unwrap();
	window.set_application(Some(application));
	window.set_position(gtk::WindowPosition::Center);
	window.add_events(
		gdk::EventMask::STRUCTURE_MASK
			| gdk::EventMask::BUTTON_PRESS_MASK
			| gdk::EventMask::KEY_PRESS_MASK,
	);
	let columns: gtk::SpinButton = builder.get_object("columns").unwrap();

	let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
	let library = Rc::new(Library {
		songs: xdg
			.list_data_files("library")
			.into_iter()
			.filter(|path| path.is_file())
			.map(|path| {
				(
					path.file_stem().unwrap().to_string_lossy().into_owned(),
					path,
				)
			})
			.collect(),
	});
	let image_cache = Rc::new(RefCell::new(lru_disk_cache::LruDiskCache::new(
			xdg.place_cache_file("staves_small.cache").expect("Could not create cache file"), 
			100 * 1024 * 1024
	).unwrap()));

	let state = Rc::new(RefCell::new(Option::<ViewerState>::None));
	let carousel = builder.get_object("carousel").unwrap();
	let sheet_viewer = Rc::new(RefCell::new(SheetViewer::new(&carousel)));

	let part_selection: gtk::ComboBoxText = builder.get_object("part_selection").unwrap();

	let part_selection_changed_signal = Rc::new(part_selection.connect_changed(
		clone!(@strong state, @weak carousel => @default-panic, move |part_selection| {
			let section = part_selection.get_active_id().unwrap();

			if let Some(state) = &*state.borrow() {
				carousel.scroll_to(&carousel.get_children()[
					*state.layout.get_page_of_staff(section.parse::<StaffIndex>().unwrap())
				]);
			}
		}),
	));

	let next = gio::SimpleAction::new("next_page", None);
	next.connect_activate(
		clone!(@strong state, @strong carousel => @default-panic, move |_action, _value| {
			if state.borrow().is_some() {
				carousel.scroll_to(&carousel.get_children()[
					usize::min(carousel.get_position() as usize + 1, carousel.get_n_pages() as usize - 1)
				]);
			}
		}),
	);
	application.add_action(&next);
	application.set_accels_for_action("app.next_page", &["<Primary>N", "<Alt>N", "Right"]);

	let previous = gio::SimpleAction::new("previous_page", None);
	previous.connect_activate(
		clone!(@strong state, @strong carousel => @default-panic, move |_action, _value| {
			if let Some(state) = &*state.borrow() {
				carousel.scroll_to(&carousel.get_children()[
					*state.go_back(PageIndex(carousel.get_position() as usize))
						.unwrap_or_else(|| PageIndex(usize::max(carousel.get_position() as usize, 1) - 1))
				]);
			}
		}),
	);
	application.add_action(&previous);
	application.set_accels_for_action("app.previous_page", &["<Primary>P", "<Alt>P", "Left"]);

	if !EXPERIMENT_MODE {
		let zoom_gesture = &sheet_viewer.borrow().zoom_gesture;
		zoom_gesture.connect_begin(clone!(@strong state => move |_, _| {
			println!("Begin");
			if let Some(state) = &mut *state.borrow_mut() {
				state.zoom_before_gesture = Some(state.zoom);
			}
		}));

		zoom_gesture.connect_end(clone!(@strong state => move |_, _| {
			println!("End");
			if let Some(state) = &mut *state.borrow_mut() {
				state.zoom_before_gesture = None;
			}
		}));

		zoom_gesture.connect_cancel(clone!(@strong state => move |_, _| {
			println!("Cancel");
			if let Some(state) = &mut *state.borrow_mut() {
				state.zoom = state.zoom_before_gesture.take()
					//.expect("Should always be Some within after gesture started");
					.unwrap_or(state.zoom)
			}
		}));

		zoom_gesture.connect_scale_changed(clone!(@strong state, @strong sheet_viewer, @weak carousel => @default-panic, move |_, scale| {
			if let Some(state) = &mut *state.borrow_mut() {
				dbg!(scale);
				let new_zoom = scale * state.zoom_before_gesture.expect("Should always be Some within after gesture started");
				// TODO replace with clamp once stable
				let new_zoom = f64::max(0.2, f64::min(1.0, new_zoom));
				state.change_size(carousel.get_allocated_width() as f64, 
					carousel.get_allocated_height() as f64, state.columns, new_zoom);
			}
			SheetViewer::update(&sheet_viewer, &state);
		}));
	}

	{
		carousel.connect_page_changed(clone!(@strong state, @weak part_selection, @strong part_selection_changed_signal => move |_carousel, page| {
			use std::ops::DerefMut;
			if let Ok(mut state) = state.try_borrow_mut() {
				if let Some(state) = state.deref_mut() {
					state.page = PageIndex(page as usize);
					part_selection.block_signal(&part_selection_changed_signal);
					part_selection.set_active_id(
						Some(&state.part_start(state.page).to_string())
					);
					part_selection.unblock_signal(&part_selection_changed_signal);
				}
			}
		}));
		carousel.connect_key_press_event(
			clone!(@weak next, @weak previous => @default-panic, move |_carousel, event| {
				use gdk::keys::constants;
				match event.get_keyval() {
					constants::Right | constants::KP_Right | constants::space | constants::KP_Space => {
						next.activate(None);
						gtk::Inhibit(true)
					},
					constants::Left | constants::KP_Left | constants::BackSpace => {
						previous.activate(None);
						gtk::Inhibit(true)
					}
					_ => gtk::Inhibit(false)
				}
			}),
		);
		// TODO make this work and remove the other event handler
		// carousel.connect_size_allocate(clone!(@strong state, @weak columns => @default-panic, move |carousel, event| {
		// if let Some(state) = &mut *state.borrow_mut() {
		// 	state.change_size(&library, event.width as f64, event.height as f64, columns.get_value() as usize);
		// }
		//rebuild_carousel(&carousel, &*state.borrow(), &library);
		// }));
		window.connect_configure_event(clone!(@strong state, @strong sheet_viewer, @weak columns, @strong carousel => @default-panic, move |_, event| {
			if let Some(state) = &mut *state.borrow_mut() {
				state.change_size(carousel.get_allocated_width() as f64, 
					carousel.get_allocated_height() as f64, columns.get_value() as usize, state.zoom);
			}
			SheetViewer::update(&sheet_viewer, &state);
			false
		}));
	}

	columns.connect_property_value_notify(clone!(@strong state, @strong sheet_viewer, @strong carousel => @default-panic, move|columns| {
		if let Some(state) = &mut *state.borrow_mut() {
			state.change_size(carousel.get_allocated_width() as f64, carousel.get_allocated_height() as f64, columns.get_value() as usize, state.zoom);
		}
		SheetViewer::update(&sheet_viewer, &state);
	}));

	let deck: libhandy::Deck = builder.get_object("deck").unwrap();

	{
		/* Go back handling */
		let go_back_action = gio::SimpleAction::new("go-back", None);
		application.add_action(&go_back_action);
		go_back_action.connect_activate(
			clone!(@strong state, @weak deck => @default-panic, move |_action, _no_value| {
				*state.borrow_mut() = None;
				deck.navigate(libhandy::NavigationDirection::Back);
			}),
		);
	}

	{
		/* Song selection */
		let store_songs: gtk::ListStore = builder.get_object("store_songs").unwrap();
		let library_grid: gtk::IconView = builder.get_object("library_grid").unwrap();

		for (i, (name, path)) in library.songs.iter().enumerate() {
			store_songs.set(&store_songs.append(), &[1], &[&name.to_value()]);

			let path = path.clone();
			let store_songs = store_songs.clone();
			glib::MainContext::default().spawn_local_with_priority(
				glib::source::PRIORITY_DEFAULT_IDLE,
				async move {
					let preview_image = Song::load_first_staff(&path).await;
					if let Some(preview_image) = preview_image {
						store_songs.set(
							&store_songs.iter_nth_child(None, i as i32).unwrap(),
							&[0],
							&[&preview_image],
						);
					}
				},
			);
		}

		library_grid.connect_item_activated(clone!(@strong state, @strong sheet_viewer, @strong library, @weak columns, @weak carousel, @weak part_selection, @strong part_selection_changed_signal, @strong deck => @default-panic, move |_libhandy_grid, item| {
			let state = state.clone();
			let sheet_viewer = sheet_viewer.clone();
			let library = library.clone();
			let deck = deck.clone();
			let part_selection_changed_signal = part_selection_changed_signal.clone();
			let image_cache = image_cache.clone();

			let text = store_songs.get_value(&store_songs.get_iter(item).unwrap(), 1)
				.get::<glib::GString>()
				.unwrap()
				.unwrap();
			glib::MainContext::default().spawn_local_with_priority(glib::source::PRIORITY_DEFAULT_IDLE, async move {
				let progress = create_progress_spinner_dialog();
	
				*state.borrow_mut() = Some(
					async move {
						ViewerState::new(
							library.load_song(&text.as_str(), image_cache).await,
							columns.get_value() as usize,
							carousel.get_allocated_width() as f64,
							carousel.get_allocated_height() as f64
						)
					}.await
				);
	
				part_selection.block_signal(&part_selection_changed_signal);
				part_selection.remove_all();
				if let Some(state) = &*state.borrow() {
					let parts = state.get_parts();
					for (k, p) in &parts {
						part_selection.append(Some(&k.to_string()), p);
					}
					let relevant = parts.len() > 1;
					part_selection.set_active(if relevant {Some(0)} else {None});
					part_selection.set_sensitive(relevant);
				} else {
					part_selection.set_active(None);
					part_selection.set_sensitive(false);
				}
				part_selection.unblock_signal(&part_selection_changed_signal);

				deck.navigate(libhandy::NavigationDirection::Forward);
				progress.emit_close();

				/* This will spawn its own async Future, as that one might easily get cancelled */
				SheetViewer::update(&sheet_viewer, &state);
			});
		}));
	}

	{
		/* Full screen handling */
		enum HeaderState {
			WindowedAlwaysShow,
			FullscreenShowTimeout,
			FullscreenShowFocus,
			FullscreenHidden,
		};
		let is_fullscreen = Rc::new(std::cell::Cell::new(false));

		let revealer: gtk::Revealer = builder.get_object("revealer").unwrap();
		let header: libhandy::HeaderBar = builder.get_object("header").unwrap();
		let fullscreen_button: gtk::Button = builder.get_object("fullscreen").unwrap();
		let restore_button: gtk::Button = builder.get_object("restore").unwrap();

		let enter_fullscreen = gio::SimpleAction::new("enter_fullscreen", None);
		application.add_action(&enter_fullscreen);
		application.set_accels_for_action("app.enter_fullscreen", &["F11"]);

		enter_fullscreen.connect_activate(
			clone!(@weak window => @default-panic, move |_action, _value| {
				println!("Enter fullscreen");
				window.fullscreen();
			}),
		);

		let leave_fullscreen = gio::SimpleAction::new("leave_fullscreen", None);
		application.add_action(&leave_fullscreen);
		application.set_accels_for_action("app.leave_fullscreen", &["Escape"]);

		leave_fullscreen.connect_activate(
			clone!(@weak window => @default-panic, move |_action, _value| {
				println!("Leave fullscreen");
				window.unfullscreen();
			}),
		);

		window.connect_window_state_event(move |window, state| {
			if state
				.get_changed_mask()
				.contains(gdk::WindowState::FULLSCREEN)
			{
				if state
					.get_new_window_state()
					.contains(gdk::WindowState::FULLSCREEN)
				{
					println!("Going fullscreen");
					is_fullscreen.set(true);
					fullscreen_button.set_visible(false);
					restore_button.set_visible(true);
					header.set_show_close_button(false);

					window.queue_draw();
				} else {
					println!("Going unfullscreen");
					is_fullscreen.set(false);
					restore_button.set_visible(false);
					fullscreen_button.set_visible(true);
					header.set_show_close_button(true);
					window.queue_draw();
				}
			}
			gtk::Inhibit(false)
		});
	}

	window.show_all();

	let (midi_tx, midi_rx) =
		glib::MainContext::channel::<pedal::PageEvent>(glib::Priority::default());
	let handler = pedal::run(midi_tx).unwrap();
	midi_rx.attach(None, move |event| {
		// Reference the MIDI handler which holds the Sender so that it doesn't get dropped.
		let _handler = &handler;
		match event {
			pedal::PageEvent::Next => {
				next.activate(None);
				Continue(true)
			},
			pedal::PageEvent::Previous => {
				previous.activate(None);
				Continue(true)
			},
		}
	});
}

fn create_progress_spinner_dialog() -> gtk::Dialog {
	let progress = gtk::Dialog::new();
	progress.set_modal(true);
	progress.set_skip_taskbar_hint(true);
	progress.set_destroy_with_parent(true);
	progress.set_position(gtk::WindowPosition::CenterOnParent);
	progress.get_content_area().add(&{
		let spinner = gtk::Spinner::new();
		spinner.start();
		spinner.show();
		spinner
	});
	progress.set_title("Loading…");
	progress.set_deletable(false);
	progress.show_all();
	progress
}

mod pedal;

fn main() {
	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.expect("Initialization failed...");

	// let editor = true;
	application.connect_activate(move |app| {
		build_ui(app);
	});

	// When activated, shuts down the application
	let quit = gio::SimpleAction::new("quit", None);
	quit.connect_activate(
		clone!(@weak application => @default-panic, move |_action, _parameter| {
			application.quit();
		}),
	);
	application.add_action(&quit);
	application.connect_startup(|application| {
		libhandy::init();
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
	});

	// application.run(&args().collect::<Vec<_>>());
	application.run(&[]);
}

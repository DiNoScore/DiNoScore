#![allow(unused_imports)]
#![allow(dead_code)]

use std::sync::Arc;
use std::cell::RefCell;
use std::rc::Rc;
use gtk::prelude::*;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use libhandy::prelude::*;
/* Weird that this is required for it to work */
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;
use dinoscore::*;

struct SongRenderer {
	pdf: poppler::PopplerDocument,
	song: Arc<collection::SongMeta>,
}

impl SongRenderer {
	fn spawn(song: Arc<collection::SongMeta>, pdf: owned::OwnedPopplerDocument, actor: actix::Addr<SongActor>) -> Sender<UpdateLayout> {
		let (tx, rx) = channel();
		std::thread::spawn(|| {
			let pdf = pdf.into_inner();
			SongRenderer { pdf, song }.run(rx, actor);
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
			for num in 0..update.layout.pages.len() {
				let page = &update.layout.pages[num];
				/* Short circuit if there is newer data to be processed */
				if let Some(new_update) = fetch_latest(&rx) {
					update = new_update;
					continue 'outer;
				}
				let surface = self.render_page(page, update.width, update.height);
				actor.try_send(UpdatePage {
					index: collection::PageIndex(num),
					surface: unsafe_send_sync::UnsafeSend::new(surface),
					song: self.song.version_uuid,
				}).unwrap();
			}

			/* Next update */
			update = match rx.recv() {
				Ok(update) => update,
				Err(_) => return,
			};
		}
	}

	fn render_staff(&mut self, staff: collection::StaffIndex, width: f64) -> cairo::ImageSurface {
		let staff = &self.song.staves[*staff];
		let page = &self.pdf.get_page(*staff.page).unwrap();

		let line_width = staff.width();
		let _line_height = staff.height();
		let aspect_ratio = staff.aspect_ratio();

		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width as i32, (width * aspect_ratio) as i32).unwrap();
		let context = cairo::Context::new(&surface);

		let scale = surface.get_width() as f64 / line_width;
		context.scale(scale, scale);
		context.translate( -staff.start.0, -staff.start.1 );
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint();
		page.render(&context);

		surface.flush();
		surface
	}

	fn render_page(&mut self, page: &Vec<layout::StaffLayout>, width: i32, height: i32) -> cairo::ImageSurface {
		let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width, height).unwrap();
		let context = cairo::Context::new(&surface);
		context.set_source_rgb(0.0, 1.0, 1.0);
		context.paint();

		page.iter().for_each(|staff_layout| {
			let img = self.render_staff(staff_layout.index, staff_layout.width);
			let scale = staff_layout.width / img.get_width() as f64;

			context.save();
			context.translate(staff_layout.x, staff_layout.y);
			context.scale(scale, scale);

			/* Staff */
			context.set_source_surface(&img, 0.0, 0.0);
			context.paint();

			/* Staff number */
			context.save();
			context.set_font_size(20.0);
			context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
			context.move_to(10.0, 16.0);
			context.show_text(&staff_layout.index.to_string());
			context.restore();

			context.restore();
		});

		surface.flush();
		// surface.write_to_png(&mut std::fs::File::create("./page.png").unwrap()).unwrap();
		surface
	}
}

struct UpdateLayout {
	layout: Arc<layout::PageLayout>,
	current_page: i32,
	width: i32,
	height: i32,
}

#[derive(Debug)]
struct SongState {
	song: Arc<collection::SongMeta>,
	page: collection::PageIndex,
	layout: Arc<layout::PageLayout>,
	renderer: Sender<UpdateLayout>,
	/* To keep the current view consistent between layout changes */
	columns: usize,
	zoom: f64,
	/* Backup for when a gesture starts */
	zoom_before_gesture: Option<f64>,
}

impl SongState {
	fn new(renderer: Sender<UpdateLayout>, song: Arc<collection::SongMeta>, columns: usize, width: f64, height: f64) -> Self {
		let layout = Arc::new(layout::PageLayout::new(&song, width, height, 1.0, columns, 10.0));
		Self {
			song,
			page: 0.into(),
			layout,
			renderer,
			columns,
			zoom: 1.0,
			zoom_before_gesture: None,
		}
	}

	fn change_size(&mut self, width: f64, height: f64, columns: usize, zoom: f64) {
		self.columns = columns;
		self.zoom = zoom;
		let layout_staff = self.layout.get_center_staff(self.page);
		self.layout = Arc::new(layout::PageLayout::new(&self.song, width, height, zoom, self.columns, 10.0));
		self.page = self.layout.get_page_of_staff(layout_staff);
	}

	fn get_parts(&self) -> Vec<(collection::StaffIndex, String)> {
		self.song
			.piece_starts
			.iter()
			.map(|(k, v)| (*k, v.clone().unwrap_or_else(|| format!("({})", k))))
			.collect()
	}

	/* When we're at a given page and want to go back, should we jump to the start of the repetition? */
	fn go_back(&self, current_page: collection::PageIndex) -> Option<collection::PageIndex> {
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
	fn part_start(&self, current_page: collection::PageIndex) -> collection::StaffIndex {
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

struct SongActor {
	widgets: SongWidgets,
	application: gtk::Application,
	/// Always Some once started. Could have used a lazy init cell as well
	part_selection_changed_signal: Option<glib::SignalHandlerId>,
	// TODO remove the image surface, and manipulate the draw closure instead
	pages: Vec<Rc<(RefCell<Option<cairo::ImageSurface>>, gtk::DrawingArea)>>,
	// layout
	song: Option<SongState>,
	next: gio::SimpleAction,
	previous: gio::SimpleAction,
}

#[derive(woab::WidgetsFromBuilder)]
struct SongWidgets {
	carousel: libhandy::Carousel,
	deck: libhandy::Deck,
	part_selection: gtk::ComboBoxText,
}

impl actix::Actor for SongActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		self.application.add_action(&self.next);
		self.application.set_accels_for_action("app.next_page", &["<Primary>N", "<Alt>N", "Right"]);
		woab::connect_signal_handler::<Self, _, _>(&self.next, "activate", "Next", ctx);
	
		self.application.add_action(&self.previous);
		self.application.set_accels_for_action("app.previous_page", &["<Primary>P", "<Alt>P", "Left"]);
		woab::connect_signal_handler::<Self, _, _>(&self.previous, "activate", "Previous", ctx);

		let go_back_action = gio::SimpleAction::new("go-back", None);
		woab::connect_signal_handler::<Self, _, _>(&go_back_action, "activate", "GoBack", ctx);
		self.application.add_action(&go_back_action);

		let signal_handler = woab::make_signal_handler::<Self, _>("SelectPart", ctx);
		self.part_selection_changed_signal = Some(
			self.widgets.part_selection.connect_local("changed".as_ref(), false, signal_handler).unwrap()
		);
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("SongActor Quit");
	}
}

impl SongActor {
	fn new(widgets: SongWidgets, application: gtk::Application) -> Self {
		let next = gio::SimpleAction::new("next_page", None);
		let previous = gio::SimpleAction::new("previous_page", None);
		Self { widgets, application, pages: Vec::new(), song: None, next, previous, part_selection_changed_signal: None }
	}

	fn load_song(&mut self, ctx: &mut actix::Context<Self>, song: collection::SongMeta, pdf: owned::OwnedPopplerDocument) {
		use actix::AsyncContext;

		let song = Arc::new(song);
		let renderer = SongRenderer::spawn(song.clone(), pdf, ctx.address());
		let width = self.widgets.carousel.get_allocated_width();
		let height = self.widgets.carousel.get_allocated_height();

		let song = SongState::new(renderer, song, 3, width as f64, height as f64);

		let parts = song.get_parts();
		self.widgets.part_selection.remove_all();
		for (k, p) in &parts {
			self.widgets.part_selection.append(Some(&k.to_string()), p);
		}
		let relevant = parts.len() > 1;
		self.widgets.part_selection.set_active(if relevant {Some(0)} else {None});
		self.widgets.part_selection.set_sensitive(relevant);

		self.song = Some(song);
		self.on_resize();
	}

	fn on_resize(&mut self) {
		let song = match &mut self.song {
			Some(song) => song,
			None => return,
		};
		let width = self.widgets.carousel.get_allocated_width();
		let height = self.widgets.carousel.get_allocated_height();

		song.change_size(width as f64, height as f64, 3, 1.0);

		let carousel = &self.widgets.carousel;
		carousel.foreach(|p| carousel.remove(p));
		self.pages.clear();

		for _index in 0..song.layout.pages.len() {
			let area = gtk::DrawingArea::new();
			area.set_hexpand(true);
			area.set_vexpand(true);

			let page = Rc::new((RefCell::new(None), area.clone()));
			self.pages.push(page.clone());
			area.connect_draw(move |_area, context| {
				context.set_source_rgb(1.0, 0.0, 1.0);
				context.paint();
				// println!("(DrawingArea) Render page {}, {}", index, page.0.borrow().as_ref().is_some());
				if let Some(page) = page.0.borrow().as_ref() {
					context.set_source_surface(page, 0.0, 0.0);
					context.paint();
				}
				gtk::Inhibit::default()
			});
			carousel.add(&area);
			area.show();
		}
		carousel.queue_draw();

		song.renderer.send(UpdateLayout {
			layout: song.layout.clone(),
			current_page: 0,
			width,
			height,
		}).unwrap();
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct UpdatePage {
	index: collection::PageIndex,
	surface: unsafe_send_sync::UnsafeSend<cairo::ImageSurface>,
	/// Song identifier to ignore old data
	song: uuid::Uuid,
}

impl actix::Handler<UpdatePage> for SongActor {
	type Result = ();

	fn handle(&mut self, page: UpdatePage, _ctx: &mut Self::Context) -> Self::Result {
		if let Some(song) = self.song.as_ref() {
			if page.song != song.song.version_uuid {
				return;
			}
			// println!("Updating page {}", page.index);
			self.pages[*page.index].0.replace(Some(page.surface.unwrap()));
			self.pages[*page.index].1.queue_draw();
			// dbg!(&self.pages);
			self.widgets.carousel.queue_draw();
		}
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct LoadSong {
	meta: collection::SongMeta,
	pdf: owned::OwnedPopplerDocument,
}

impl actix::Handler<LoadSong> for SongActor {
	type Result = ();

	fn handle(&mut self, song: LoadSong, ctx: &mut Self::Context) -> Self::Result {
		self.load_song(ctx, song.meta, song.pdf);
	}
}

#[derive(woab::BuilderSignal)]
enum SongEvent {
	#[signal(ret = false)]
	WindowSizeChanged,
	Next,
	Previous,
	GoBack,
	#[signal(ret = false)]
	CarouselKeyPress(libhandy::Carousel, #[signal(event)] gdk::EventKey),
	#[signal(ret = false)]
	CarouselButtonPress(libhandy::Carousel, #[signal(event)] gdk::EventButton),
	CarouselPageChanged(libhandy::Carousel, u32),
	SelectPart,
}

impl actix::StreamHandler<SongEvent> for SongActor {
	fn handle(&mut self, signal: SongEvent, _ctx: &mut Self::Context) {
		let carousel = &self.widgets.carousel;
		match signal {
			SongEvent::WindowSizeChanged => self.on_resize(),
			SongEvent::Next => {
				if self.song.is_none() {
					return;
				}
				carousel.scroll_to(&carousel.get_children()[
					usize::min(carousel.get_position() as usize + 1, carousel.get_n_pages() as usize - 1)
				]);
			},
			SongEvent::Previous => {
				if let Some(song) = self.song.as_mut() {
					carousel.scroll_to(&carousel.get_children()[
						*song.go_back(collection::PageIndex(carousel.get_position() as usize))
							.unwrap_or_else(|| collection::PageIndex(usize::max(carousel.get_position() as usize, 1) - 1))
					]);
				}
			},
			SongEvent::GoBack => {
				std::mem::drop(self.song.take());
				self.widgets.carousel.foreach(|p| self.widgets.carousel.remove(p));
				self.pages.clear();

				self.widgets.part_selection.set_active(None);
				self.widgets.part_selection.set_sensitive(false);
				self.widgets.part_selection.remove_all();

				self.widgets.deck.navigate(libhandy::NavigationDirection::Back);
			},
			// TODO add cooldown
			SongEvent::CarouselKeyPress(_carousel, event) => {
				use gdk::keys::constants;
				match event.get_keyval() {
					constants::Right | constants::KP_Right | constants::space | constants::KP_Space => {
						self.next.activate(None);
						gtk::Inhibit(true)
					},
					constants::Left | constants::KP_Left | constants::BackSpace => {
						self.previous.activate(None);
						gtk::Inhibit(true)
					}
					_ => gtk::Inhibit(false)
				}; // TODO inhibit
			},
			// TODO add cooldown
			// TODO don't trigger on top of a swipe gesture
			SongEvent::CarouselButtonPress(carousel, event) => {
				let x = event.get_position().0 / carousel.get_allocated_width() as f64;
				if (0.0..0.2).contains(&x) {
					self.previous.activate(None);
				} else if (0.8..1.0).contains(&x) {
					self.next.activate(None);
				}
			},
			SongEvent::CarouselPageChanged(_carousel, page) => {
				let song = self.song.as_mut().unwrap();
				song.page = collection::PageIndex(page as usize);
				self.widgets.part_selection.block_signal(self.part_selection_changed_signal.as_ref().unwrap());
				self.widgets.part_selection.set_active_id(
					Some(&song.part_start(song.page).to_string())
				);
				self.widgets.part_selection.unblock_signal(self.part_selection_changed_signal.as_ref().unwrap());
			},
			SongEvent::SelectPart => if self.song.is_some() {
				let section = self.widgets.part_selection.get_active_id().unwrap();

				self.widgets.carousel.scroll_to(&carousel.get_children()[
					*self.song.as_ref().unwrap()
						.layout
						.get_page_of_staff(section.parse::<collection::StaffIndex>().unwrap())
				]);
			},
		}
	}
}




struct LibraryActor {
	widgets: LibraryWidgets,
	library: Rc<RefCell<library::Library>>,
	song_actor: actix::Addr<SongActor>,
}



#[derive(woab::WidgetsFromBuilder)]
struct LibraryWidgets {
	store_songs: gtk::ListStore,
	library_grid: gtk::IconView,
	deck: libhandy::Deck,
}

impl actix::Actor for LibraryActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, _ctx: &mut Self::Context) {
		println!("Starting LibraryActor");
		/* TODO add a true loading spinner */
		let library = &self.library;
		let store_songs = &self.widgets.store_songs;
		store_songs.set_sort_column_id(gtk::SortColumn::Index(1), gtk::SortType::Ascending);

		for (i, (_uuid, song)) in library.borrow().songs.iter().enumerate() {
			// TODO clean this up
			/* Add an item with the name and UUID */
			store_songs.set(
				&store_songs.append(),
				&[1, 2],
				&[
					&song.title().unwrap_or("<no title>").to_value(),
					&song.uuid().to_string().to_value(),
				]);
			/* Optionally set the image */
			if let Some(thumbnail) = song.thumbnail() {
				/* Index, column, value */
				store_songs.set(
					&store_songs.iter_nth_child(None, i as i32).unwrap(),
					&[0],
					&[&thumbnail],
				);
			}
		}
		self.widgets.library_grid.show();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("Library Quit");
	}
}

#[derive(woab::BuilderSignal, Debug)]
enum LibrarySignal {
	LoadSong(gtk::IconView, gtk::TreePath),
}

impl actix::StreamHandler<LibrarySignal> for LibraryActor {
	fn handle(&mut self, signal: LibrarySignal, _ctx: &mut Self::Context) {
		match signal {
			LibrarySignal::LoadSong(_library_grid, item) => {
				println!("Loading song:");
				let text = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 1)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				let uuid = self.widgets.store_songs.get_value(&self.widgets.store_songs.get_iter(&item).unwrap(), 2)
					.get::<glib::GString>()
					.unwrap()
					.unwrap();
				dbg!(&text.as_str());
				dbg!(&uuid.as_str());

				self.widgets.deck.navigate(libhandy::NavigationDirection::Forward);

				let uuid = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
				let mut library = self.library.borrow_mut();
				let song = library.songs.get_mut(&uuid).unwrap();
				self.song_actor.try_send(LoadSong {
					meta: song.index.clone(),
					pdf: song.load_sheet(),
				}).unwrap();
			},
		}
	}
}

struct FullscreenActor {
	widgets: FullscreenWidgets,
	application: gtk::Application,
	is_fullscreen: bool
}

#[derive(woab::WidgetsFromBuilder)]
struct FullscreenWidgets {
	window: gtk::ApplicationWindow,
	header: libhandy::HeaderBar,
	#[widget(name = "fullscreen")]
	fullscreen_button: gtk::Button,
	#[widget(name = "restore")]
	restore_button: gtk::Button,
}

#[derive(Debug, woab::BuilderSignal)]
enum FullscreenSignal {
	Fullscreen,
	Unfullscreen,
	#[signal(ret = false)]
	WindowState(gtk::Window, #[signal(event)] gdk::EventWindowState),
}

impl actix::Actor for FullscreenActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let application = &self.application;

		let enter_fullscreen = gio::SimpleAction::new("enter_fullscreen", None);
		application.add_action(&enter_fullscreen);
		application.set_accels_for_action("app.enter_fullscreen", &["F11"]);

		woab::connect_signal_handler::<Self, _, _>(&enter_fullscreen, "activate", "Fullscreen", ctx);

		let leave_fullscreen = gio::SimpleAction::new("leave_fullscreen", None);
		application.add_action(&leave_fullscreen);
		application.set_accels_for_action("app.leave_fullscreen", &["Escape"]);
		leave_fullscreen.connect_local("activate", false, woab::make_signal_handler::<Self, _>("Unfullscreen", ctx)).unwrap();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("Fullscreen Quit");
	}
}

impl actix::StreamHandler<FullscreenSignal> for FullscreenActor {
	fn handle(&mut self, signal: FullscreenSignal, _ctx: &mut Self::Context) {
		match signal {
			FullscreenSignal::Fullscreen => {
				println!("Enter fullscreen");
				self.widgets.window.fullscreen();
			},
			FullscreenSignal::Unfullscreen => {
				println!("Leave fullscreen");
				self.widgets.window.unfullscreen();
			},
			FullscreenSignal::WindowState(window, state) => {
				if state
					.get_changed_mask()
					.contains(gdk::WindowState::FULLSCREEN)
				{
					if state
						.get_new_window_state()
						.contains(gdk::WindowState::FULLSCREEN)
					{
						println!("Going fullscreen");
						self.is_fullscreen = true;
						self.widgets.fullscreen_button.set_visible(false);
						self.widgets.restore_button.set_visible(true);
						self.widgets.header.set_show_close_button(false);
	
						window.queue_draw();
					} else {
						println!("Going unfullscreen");
						self.is_fullscreen = false;
						self.widgets.restore_button.set_visible(false);
						self.widgets.fullscreen_button.set_visible(true);
						self.widgets.header.set_show_close_button(true);
						window.queue_draw();
					}
				}
			},
		}
	}
}

struct AppActor {
	widgets: AppWidgets,
	application: gtk::Application,
	builder: Rc<woab::BuilderConnector>,
	song_actor: actix::Addr<SongActor>,
}

#[derive(woab::WidgetsFromBuilder)]
struct AppWidgets {
	window: gtk::ApplicationWindow,
	columns: gtk::SpinButton,
	carousel: libhandy::Carousel,
	part_selection: gtk::ComboBoxText,
	deck: libhandy::Deck,
}

impl actix::Actor for AppActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let application = &self.application;
		let window = &self.widgets.window;
		// window.set_application(Some(&self.application)); // <-- This line segfaults
		window.set_position(gtk::WindowPosition::Center);
		window.add_events(
			gdk::EventMask::STRUCTURE_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::KEY_PRESS_MASK,
		);

		let quit = gio::SimpleAction::new("quit", None);
		quit.connect_activate(
			clone!(@weak application => @default-panic, move |_action, _parameter| {
				println!("Quit for real");
				application.quit();
			}),
		);
		application.add_action(&quit);
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
		window.connect_destroy(clone!(@weak application => @default-panic, move |_| {
			println!("Destroy quit");
			application.quit();
		}));

		window.show_all();

		use actix::AsyncContext;
		let addr = ctx.address();
		/* Spawn library actor once library is loaded */
		// std::thread::spawn(move || {
			// let addr = addr;
			// println!("Loading library");
			// let library = futures::executor::block_on(library::Library::load()).unwrap();
			// println!("Loaded library");
			// addr.try_send(CreateLibraryActor(library)).unwrap();
		// });
		use actix::Handler;
		// self.handle(CreateLibraryActor(library), ctx);
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		println!("Actor Quit");
		// gtk::main_quit();
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct CreateLibraryActor(library::Library);

impl actix::Handler<CreateLibraryActor> for AppActor {
	type Result = ();

	fn handle(&mut self, msg: CreateLibraryActor, ctx: &mut Self::Context) -> Self::Result {
		let library = msg.0;
		self.builder.actor()
			.connect_signals::<LibrarySignal>()
			.create(|_ctx| {
				LibraryActor {
					widgets: self.builder.widgets().unwrap(),
					library: Rc::new(RefCell::new(library)),
					song_actor: self.song_actor.clone(),
				}
			});
	}
}

#[derive(woab::BuilderSignal, Debug)]
enum AppSignal {
	// WindowDestroy
}

impl actix::StreamHandler<AppSignal> for AppActor {
	fn handle(&mut self, signal: AppSignal, _ctx: &mut Self::Context) {
		println!("A: {:?}", signal);
		// match signal {
		// 	AppSignal::WindowDestroy => {},
		// }
	}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.expect("Initialization failed...");

	application.connect_startup(|_application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();
		woab::run_actix_inside_gtk_event_loop("my-WoAB-app").unwrap(); // <===== IMPORTANT!!!
		println!("Woab started");
	});

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/viewer.glade");
		let builder = Rc::new(woab::BuilderConnector::from(builder));

		let song_actor = builder.actor()
			.connect_signals::<SongEvent>()
			.create(|_ctx| SongActor::new(builder.widgets().unwrap(), application.clone()));

		builder.actor()
			.connect_signals::<FullscreenSignal>()
			.create(|_ctx| {
				FullscreenActor {
					widgets: builder.widgets().unwrap(),
					application: application.clone(),
					is_fullscreen: false,
				}
			});

		builder.actor()
			.connect_signals::<LibrarySignal>()
			.create(|_ctx| {
				println!("Loading library");
				let library = futures::executor::block_on(library::Library::load()).unwrap();
				println!("Loaded library");
				LibraryActor {
					widgets: builder.widgets().unwrap(),
					library: Rc::new(RefCell::new(library)),
					song_actor: song_actor.clone(),
				}
			});

		builder.actor()
			.connect_signals::<AppSignal>()
			.create({
				let builder = &builder;
				clone!(@weak application, @strong song_actor => @default-panic, move |_ctx| {
					let widgets: AppWidgets = builder.widgets().unwrap();
					widgets.window.set_application(Some(&application));
					AppActor {
						widgets,
						application,
						song_actor,
						builder: builder.clone(),
					}
				})
			});
	});

	application.run(&[]);
	Ok(())
}

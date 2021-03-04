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

pub fn create(builder: &woab::BuilderConnector, application: gtk::Application) -> actix::Addr<SongActor> {
	builder.actor()
			.connect_signals(SongEvent::connector().inhibit(SongEvent::inhibit))
			.create(|_ctx| SongActor::new(builder.widgets().unwrap(), application))
}

struct SongRenderer {
	pdf: poppler::PopplerDocument,
	song: Arc<collection::SongMeta>,
	image_cache: lru_disk_cache::LruDiskCache,
}

impl SongRenderer {
	fn spawn(song: Arc<collection::SongMeta>, pdf: owned::OwnedPopplerDocument, actor: actix::Addr<SongActor>) -> Sender<UpdateLayout> {
		// TODO move that somewhere else
		let xdg = xdg::BaseDirectories::with_prefix("dinoscore").unwrap();
		let image_cache = lru_disk_cache::LruDiskCache::new(
			xdg.place_cache_file("staves_small.cache")
				.expect("Could not create cache file"),
			100 * 1024 * 1024,
		).unwrap();

		let (tx, rx) = channel();
		std::thread::spawn(|| {
			let pdf = pdf.into_inner();
			SongRenderer { pdf, song, image_cache }.run(rx, actor);
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
			for mut num in 0..update.layout.pages.len() {
				/* Do the currently visible page first, by swapping the indices */
				if num == 0 {
					num = *update.current_page;
				} else if num == *update.current_page {
					num = 0;
				}

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
		/*
		 * MIP mapping. For the given width, calculate the actual width we want this to be rendered.
		 * We will never scale an image up and never scale it down more than 2/3
		 */
		let rendered_width = (1.5f64).powf(width.log(1.5).ceil()).ceil();
		// println!("Input width {}, output width {}", width, rendered_width);
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
			// println!("Cache hit for {}", cache_key.to_string_lossy());
			let mut read = self.image_cache
				.get(&cache_key)
				.unwrap();
			cairo::ImageSurface::create_from_png(&mut read)
				.unwrap_or_else(|e| {
					println!("That cache image seems to be corrupt {}", e);
					/* Remove the corrupt entry and try again */
					self.image_cache.remove(&cache_key).unwrap();
					self.render_staff(staff, width)
				})
		} else {
			// println!("Cache miss for {}", cache_key.to_string_lossy());
			let staff = &self.song.staves[*staff];
			let page = &self.pdf.get_page(*staff.page).unwrap();
	
			let line_width = staff.width();
			let _line_height = staff.height();
			let aspect_ratio = staff.aspect_ratio();
	
			let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, rendered_width as i32, (rendered_width * aspect_ratio) as i32).unwrap();
			let context = cairo::Context::new(&surface);
	
			let scale = surface.get_width() as f64 / line_width;
			context.scale(scale, scale);
			context.translate( -staff.start.0, -staff.start.1 );
			context.set_source_rgb(1.0, 1.0, 1.0);
			context.paint();
			page.render(&context);
	
			surface.flush();
			self.image_cache
				.insert_with(&cache_key, |mut file| {
					surface.write_to_png(&mut file)
						.unwrap_or_else(|e| {
							println!("Could not write image to cache, {}", e);
						});
					Ok(())
				})
				.unwrap();

			surface
		}
	}

	fn render_page(&mut self, page: &Vec<layout::StaffLayout>, width: i32, height: i32) -> cairo::ImageSurface {
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
		surface
	}
}

struct UpdateLayout {
	layout: Arc<layout::PageLayout>,
	current_page: collection::PageIndex,
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

pub struct SongActor {
	widgets: SongWidgets,
	application: gtk::Application,
	/// Always Some once started. Could have used a lazy init cell as well
	part_selection_changed_signal: Option<glib::SignalHandlerId>,
	// layout
	song: Option<SongState>,
	next: gio::SimpleAction,
	previous: gio::SimpleAction,
}

#[derive(woab::WidgetsFromBuilder)]
struct SongWidgets {
	header: libhandy::HeaderBar,
	carousel: libhandy::Carousel,
	deck: libhandy::Deck,
	part_selection: gtk::ComboBoxText,
}

impl actix::Actor for SongActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let connector = SongEvent::connector().route_to::<Self>(ctx);

		self.application.add_action(&self.next);
		self.application.set_accels_for_action("app.next_page", &["<Primary>N", "<Alt>N", "Right"]);
		connector.connect(&self.next, "activate", "Next").unwrap();

		self.application.add_action(&self.previous);
		self.application.set_accels_for_action("app.previous_page", &["<Primary>P", "<Alt>P", "Left"]);
		connector.connect(&self.previous, "activate", "Previous").unwrap();

		let go_back_action = gio::SimpleAction::new("go-back", None);
		self.application.add_action(&go_back_action);
		connector.connect(&go_back_action, "activate", "GoBack").unwrap();

		let signal_handler = connector.handler("SelectPart").unwrap();
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
		Self { widgets, application, song: None, next, previous, part_selection_changed_signal: None }
	}

	fn load_song(&mut self, ctx: &mut actix::Context<Self>, song: collection::SongMeta, pdf: owned::OwnedPopplerDocument) {
		use actix::AsyncContext;

		let song = Arc::new(song);
		let renderer = SongRenderer::spawn(song.clone(), pdf, ctx.address());
		let width = self.widgets.carousel.get_allocated_width();
		let height = self.widgets.carousel.get_allocated_height();

		self.widgets.header.set_title(
			song.title.as_ref()
				.map(|title| format!("{} – DiNoScore", title))
				.as_deref()
				.or(Some("DiNoScore"))
		);

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
		// TODO fix that the allocated size initially is 1
		let width = self.widgets.carousel.get_allocated_width().max(10);
		let height = self.widgets.carousel.get_allocated_height().max(10);

		song.change_size(width as f64, height as f64, 3, 1.0);

		let carousel = &self.widgets.carousel;
		let new_pages = song.layout.pages.len();
		let old_pages = carousel.get_n_pages() as usize;
		use std::cmp::Ordering;
		match new_pages.cmp(&old_pages) {
			Ordering::Equal => {
				/* Be happy and do nothing */
			},
			Ordering::Greater => {
				/* Add missing pages */
				for _ in old_pages..new_pages {
					let area = gtk::DrawingArea::new();
					area.set_hexpand(true);
					area.set_vexpand(true);
		
					// area.connect_draw(move |_area, context| {
					// 	context.set_source_rgb(1.0, 0.0, 1.0);
					// 	context.paint();
					// 	gtk::Inhibit::default()
					// });
		
					carousel.add(&area);
					area.show();
				}
			},
			Ordering::Less => {
				/* Remove excess pages */
				for page in &carousel.get_children()[new_pages..old_pages] {
					carousel.remove(page);
				}
			},
		}

		carousel.queue_draw();
		carousel.scroll_to_full(&carousel.get_children()[*song.page], 0);

		song.renderer.send(UpdateLayout {
			layout: song.layout.clone(),
			current_page: song.page,
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
			let area = &self.widgets.carousel.get_children()[*page.index];
			let area: &gtk::DrawingArea = area.downcast_ref().unwrap();
			let surface = page.surface.unwrap();
			area.connect_draw(move |area, context| {
				// context.set_source_rgb(1.0, 0.0, 1.0);
				// context.paint();
				if surface.get_width() != area.get_allocated_width() 
				|| surface.get_height() != area.get_allocated_height()  {
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
	pub pdf: owned::OwnedPopplerDocument,
}

impl actix::Handler<LoadSong> for SongActor {
	type Result = ();

	fn handle(&mut self, song: LoadSong, ctx: &mut Self::Context) -> Self::Result {
		self.load_song(ctx, song.meta, song.pdf);
	}
}

#[derive(woab::BuilderSignal)]
enum SongEvent {
	#[signal(inhibit = false)]
	WindowSizeChanged,
	Next,
	Previous,
	GoBack,
	CarouselKeyPress(libhandy::Carousel, #[signal(event)] gdk::EventKey),
	CarouselButtonPress(libhandy::Carousel, #[signal(event)] gdk::EventButton),
	CarouselPageChanged(libhandy::Carousel, u32),
	SelectPart,
}

impl SongEvent {
	fn inhibit(&self) -> Option<gtk::Inhibit> {
		match self {
			Self::CarouselKeyPress(_carousel, event) => {
				use gdk::keys::constants;
				Some(match event.get_keyval() {
					constants::Right | constants::KP_Right | constants::space | constants::KP_Space => {
						gtk::Inhibit(true)
					},
					constants::Left | constants::KP_Left | constants::BackSpace => {
						gtk::Inhibit(true)
					},
					_ => gtk::Inhibit(false),
				})
				// Some(gtk::Inhibit(false))
			},
			SongEvent::CarouselButtonPress(carousel, event) => {
				let x = event.get_position().0 / carousel.get_allocated_width() as f64;
				Some(gtk::Inhibit((0.0..0.2).contains(&x) || (0.8..1.0).contains(&x)))
			},
			_ => None,
		}
	}
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
				if let Some(song) = self.song.as_ref() {
					carousel.scroll_to(&carousel.get_children()[
						*song.go_back(collection::PageIndex(carousel.get_position() as usize))
							.unwrap_or_else(|| collection::PageIndex(usize::max(carousel.get_position() as usize, 1) - 1))
					]);
				}
			},
			SongEvent::GoBack => {
				std::mem::drop(self.song.take());
				self.widgets.carousel.foreach(|p| self.widgets.carousel.remove(p));

				self.widgets.part_selection.set_active(None);
				self.widgets.part_selection.set_sensitive(false);
				self.widgets.part_selection.remove_all();

				self.widgets.deck.navigate(libhandy::NavigationDirection::Back);
			},
			// TODO add cooldown
			SongEvent::CarouselKeyPress(_carousel, event) => {
				if self.song.is_none() {
					return;
				}
				use gdk::keys::constants;
				match event.get_keyval() {
					constants::Right | constants::KP_Right | constants::space | constants::KP_Space => {
						self.next.activate(None);
					},
					constants::Left | constants::KP_Left | constants::BackSpace => {
						self.previous.activate(None);
					},
					_ => {},
				}
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

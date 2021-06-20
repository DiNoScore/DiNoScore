#![allow(unused_imports)]
#![allow(dead_code)]

use actix::Actor;
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use libhandy::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
/* Weird that this is required for it to work */
use anyhow::Context;
use dinoscore::*;
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;

mod fullscreen_actor;
mod library_actor;
mod mouse_actor;
mod pedal;
mod song_actor;
mod xournal;

use fullscreen_actor::FullscreenActor;
use library_actor::LibraryActor;
use song_actor::SongActor;

struct AppActor {
	widgets: AppWidgets,
	application: gtk::Application,
	// builder: Rc<woab::BuilderConnector>,
	song_actor: actix::Addr<SongActor>,
}

#[derive(woab::WidgetsFromBuilder)]
struct AppWidgets {
	window: gtk::ApplicationWindow,
	carousel: libhandy::Carousel,
	part_selection: gtk::ComboBoxText,
	deck: libhandy::Deck,
}

impl actix::Actor for AppActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, _ctx: &mut Self::Context) {
		let application = &self.application;
		let window = &self.widgets.window;
		// window.set_application(Some(&self.application)); // <-- This line segfaults
		window.set_position(gtk::WindowPosition::Center);
		window.add_events(
			gdk::EventMask::STRUCTURE_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::POINTER_MOTION_MASK,
		);

		let quit = gio::SimpleAction::new("quit", None);
		quit.connect_activate(
			clone!(@weak application => @default-panic, move |_action, _parameter| {
				log::debug!("Quit for real");
				application.quit();
			}),
		);
		application.add_action(&quit);
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
		window.connect_destroy(clone!(@weak application => @default-panic, move |_| {
			log::debug!("Destroy quit");
			application.quit();
		}));

		window.show_all();

		// use actix::AsyncContext;
		// let addr = ctx.address();
		/* Spawn library actor once library is loaded */
		// std::thread::spawn(move || {
		// let addr = addr;
		// log::debug!("Loading library");
		// let library = futures::executor::block_on(library::Library::load()).unwrap();
		// log::debug!("Loaded library");
		// addr.try_send(CreateLibraryActor(library)).unwrap();
		// });
		// use actix::Handler;
		// self.handle(CreateLibraryActor(library), ctx);
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Actor Quit");
		// gtk::main_quit();
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct CreateLibraryActor(library::Library);

// impl actix::Handler<CreateLibraryActor> for AppActor {
// 	type Result = ();

// 	fn handle(&mut self, msg: CreateLibraryActor, ctx: &mut Self::Context) -> Self::Result {
// 		let library = msg.0;
// 		let library_actor = library_actor::create(&self.builder, self.song_actor.clone(), library);
// 	}
// }

impl actix::Handler<woab::Signal> for AppActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, _signal: woab::Signal, _ctx: &mut Self::Context) -> woab::SignalResult {
		unreachable!();
	}
}

#[allow(clippy::unnecessary_wraps)]
fn main() -> anyhow::Result<()> {
	simple_logger::SimpleLogger::new()
		.with_level(log::LevelFilter::Trace)
		.init()
		.context("Failed to initialize logger")?;
	let orig_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic_info| {
		// invoke the default handler and exit the process
		orig_hook(panic_info);
		std::process::exit(1);
	}));

	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.context("Initialization failed")?;

	application.connect_startup(|application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();

		woab::run_actix_inside_gtk_event_loop().unwrap(); // <===== IMPORTANT!!!
		log::info!("Woab started");
	});

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/viewer.glade");
		let builder = woab::BuilderConnector::from(builder);

		woab::block_on(async {
			use actix::AsyncContext;

			let fullscreen_actor = fullscreen_actor::create(&builder, application.clone());
			let library = library::Library::load().unwrap();

			let hide_mouse_actor = mouse_actor::create(&builder);

			/* TODO clean this up once we figured out a less messy way to initialize
			 * cross-dependent actors. Don't forget to make the unused types private again
			 * after cleanup.
			 */
			song_actor::SongActor::create(move |ctx1| {
				let song_actor = ctx1.address();
				let library_actor = {
					let builder = &builder;
					let song_actor = song_actor.clone();
					let application = application.clone();
					LibraryActor::create(move |_ctx| LibraryActor {
						widgets: builder.widgets().unwrap(),
						application,
						library: Rc::new(RefCell::new(library)),
						song_actor,
						reference_time: std::time::SystemTime::now(),
						song_filter: Box::new(|_| true),
					})
				};

				let widgets: AppWidgets = builder.widgets().unwrap();
				let app_actor = AppActor::create(
					clone!(@weak application, @strong song_actor => @default-panic, move |_ctx| {
						widgets.window.set_application(Some(&application));
						AppActor {
							widgets,
							application,
							song_actor,
						}
					}),
				);

				let builder = builder.connect_to(
					woab::NamespacedSignalRouter::default()
						.route(song_actor)
						.route(library_actor.clone())
						.route(app_actor)
						.route(fullscreen_actor)
						.route(hide_mouse_actor),
				);

				SongActor::new(
					builder.widgets().unwrap(),
					application.clone(),
					library_actor,
				)
			});
		});
		log::info!("Application started");
	});

	application.run(&[]);
	log::info!("Thanks for using DiNoScore.");
	Ok(())
}

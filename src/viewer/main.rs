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

mod song_actor;
mod fullscreen_actor;
mod library_actor;
mod pedal;

use song_actor::SongActor;
use fullscreen_actor::FullscreenActor;
use library_actor::{LibraryActor, LibrarySignal};


struct AppActor {
	widgets: AppWidgets,
	application: gtk::Application,
	builder: Rc<woab::BuilderConnector>,
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
				| gdk::EventMask::BUTTON_PRESS_MASK,
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

		// use actix::AsyncContext;
		// let addr = ctx.address();
		/* Spawn library actor once library is loaded */
		// std::thread::spawn(move || {
			// let addr = addr;
			// println!("Loading library");
			// let library = futures::executor::block_on(library::Library::load()).unwrap();
			// println!("Loaded library");
			// addr.try_send(CreateLibraryActor(library)).unwrap();
		// });
		// use actix::Handler;
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
		library_actor::create(&self.builder, self.song_actor.clone(), library);
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
	.expect("Initialization failed...");

	application.connect_startup(|application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();

		woab::run_actix_inside_gtk_event_loop("DiNoScore").unwrap(); // <===== IMPORTANT!!!
		println!("Woab started");

		application.inhibit(
			Option::<&gtk::Window>::None,
			gtk::ApplicationInhibitFlags::IDLE,
			Some("You wouldn't want your screen go blank while playing an instrument"),
		);
	});

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/viewer.glade");
		let builder = Rc::new(woab::BuilderConnector::from(builder));

		let song_actor = song_actor::create(&*builder, application.clone());
		fullscreen_actor::create(&*builder, application.clone());
		let library = futures::executor::block_on(library::Library::load()).unwrap();
		library_actor::create(&*builder, song_actor.clone(), library);

		builder.actor()
			.connect_signals(AppSignal::connector())
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

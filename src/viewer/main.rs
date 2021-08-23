#![allow(unused_imports)]
#![allow(dead_code)]

use actix::Actor;
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use libhandy::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
/* Weird that this is required for it to work */
use anyhow::Context;
use dinoscore::*;
// use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;

mod fullscreen_actor;
mod library_actor;
mod mouse_actor;
mod pedal;
mod song_actor;
mod xournal;
mod crash_n_log;

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

fn main() -> anyhow::Result<()> {
	{ /* If we get called with an argument, show a crash dialog and exit */
		let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
		/* As usual, ignore arg0 */
		if args.len() > 1 {
			crash_n_log::show_crash_dialog(args);
			/* Never returns */
		}
	}

	crash_n_log::init()?;
	log::debug!("DiNoScore version {}.", git_version::git_version!(fallback = "unknown"));

	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	);

	application.connect_startup(|_application| {
		/* This is required so that builder can find this type. See gobject_sys::g_type_ensure */
		let _ = gio::ThemedIcon::static_type();
		libhandy::init();

		woab::run_actix_inside_gtk_event_loop();
		log::info!("Woab started");
	});

	application.connect_activate(move |application| {
		let builder = gtk::Builder::from_file("res/viewer.glade");
		let builder = woab::BuilderConnector::from(builder);

		woab::block_on(async {
			use actix::AsyncContext;

			let fullscreen_actor = fullscreen_actor::create(&builder, application.clone());

			let hide_mouse_actor = mouse_actor::create(&builder);

			let song_context = actix::Context::new();
			let song_actor = song_context.address();

			let library = library::Library::load().unwrap();
			let library_actor =
				library_actor::create(&builder, song_actor, application.clone(), library);

			let song_actor = song_actor::create(
				song_context,
				&builder,
				application.clone(),
				library_actor.clone(),
			);

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

			builder.connect_to(
				woab::NamespacedSignalRouter::default()
					.route(song_actor)
					.route(library_actor)
					.route(app_actor)
					.route(fullscreen_actor)
					.route(hide_mouse_actor),
			);
		});
		log::info!("Application started");
	});

	application.run_with_args(&[] as &[&str]);
	log::info!("Shuttign down …");
	if let Err(e) = woab::close_actix_runtime() {
		log::warn!("Failed to shut down WoAB runtime, {}", e);
	}
	log::info!("Thank you for using DiNoScore.");
	log::logger().flush();

	Ok(())
}

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

pub fn create(builder: &woab::BuilderConnector, application: gtk::Application) -> actix::Addr<FullscreenActor> {
	builder.actor()
		.connect_signals(FullscreenSignal::connector())
		.create(|_ctx| {
			FullscreenActor {
				widgets: builder.widgets().unwrap(),
				application: application.clone(),
				is_fullscreen: false,
			}
		})
}


pub struct FullscreenActor {
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
	ToggleFullscreen,
	#[signal(inhibit = false)]
	WindowState(gtk::Window, #[signal(event)] gdk::EventWindowState),
}

impl actix::Actor for FullscreenActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let connector = FullscreenSignal::connector().route_to::<Self>(ctx);
		let application = &self.application;

		let enter_fullscreen = gio::SimpleAction::new("enter_fullscreen", None);
		application.add_action(&enter_fullscreen);
		connector.connect(&enter_fullscreen, "activate", "Fullscreen").unwrap();

		let leave_fullscreen = gio::SimpleAction::new("leave_fullscreen", None);
		application.add_action(&leave_fullscreen);
		connector.connect(&leave_fullscreen, "activate", "Unfullscreen").unwrap();

		let toggle_fullscreen = gio::SimpleAction::new("toggle_fullscreen", None);
		application.add_action(&toggle_fullscreen);
		application.set_accels_for_action("app.toggle_fullscreen", &["F11"]);
		connector.connect(&toggle_fullscreen, "activate", "ToggleFullscreen").unwrap();
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
			FullscreenSignal::ToggleFullscreen => {
				println!("Toggle fullscreen");
				if self.is_fullscreen {
					self.widgets.window.unfullscreen();
				} else {
					self.widgets.window.fullscreen();
				}
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

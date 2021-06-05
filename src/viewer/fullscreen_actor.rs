use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use libhandy::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
/* Weird that this is required for it to work */
use actix::Actor;
use dinoscore::*;
use libhandy::prelude::HeaderBarExt;
use std::sync::mpsc::*;

pub fn create(
	builder: &woab::BuilderConnector,
	application: gtk::Application,
) -> actix::Addr<FullscreenActor> {
	FullscreenActor::create(|_ctx| FullscreenActor {
		widgets: builder.widgets().unwrap(),
		application: application.clone(),
		is_fullscreen: false,
	})
}

pub struct FullscreenActor {
	widgets: FullscreenWidgets,
	application: gtk::Application,
	is_fullscreen: bool,
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

impl actix::Actor for FullscreenActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		use actix::AsyncContext;
		let application = &self.application;

		let enter_fullscreen = gio::SimpleAction::new("enter_fullscreen", None);
		application.add_action(&enter_fullscreen);
		woab::route_action(&enter_fullscreen, ctx.address()).unwrap();

		let leave_fullscreen = gio::SimpleAction::new("leave_fullscreen", None);
		application.add_action(&leave_fullscreen);
		woab::route_action(&leave_fullscreen, ctx.address()).unwrap();

		let toggle_fullscreen = gio::SimpleAction::new("toggle_fullscreen", None);
		application.add_action(&toggle_fullscreen);
		application.set_accels_for_action("app.toggle_fullscreen", &["F11"]);
		woab::route_action(&toggle_fullscreen, ctx.address()).unwrap();
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Fullscreen Quit");
	}
}

impl actix::Handler<woab::Signal> for FullscreenActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, _ctx: &mut Self::Context) -> woab::SignalResult {
		signal!(match (signal) {
			"enter_fullscreen" => {
				log::info!("Enter fullscreen");
				self.widgets.window.fullscreen();
			},
			"leave_fullscreen" => {
				log::info!("Leave fullscreen");
				self.widgets.window.unfullscreen();
			},
			"toggle_fullscreen" => {
				log::info!("Toggle fullscreen");
				if self.is_fullscreen {
					self.widgets.window.unfullscreen();
				} else {
					self.widgets.window.fullscreen();
				}
			},
			"WindowState" => |window = gtk::Window, state = gdk::Event| {
				let state: gdk::EventWindowState = state.downcast().unwrap();
				if state
					.get_changed_mask()
					.contains(gdk::WindowState::FULLSCREEN)
				{
					if state
						.get_new_window_state()
						.contains(gdk::WindowState::FULLSCREEN)
					{
						log::debug!("Going fullscreen");
						self.is_fullscreen = true;
						self.widgets.fullscreen_button.set_visible(false);
						self.widgets.restore_button.set_visible(true);
						self.widgets.header.set_show_close_button(false);

						window.queue_draw();
					} else {
						log::debug!("Going unfullscreen");
						self.is_fullscreen = false;
						self.widgets.restore_button.set_visible(false);
						self.widgets.fullscreen_button.set_visible(true);
						self.widgets.header.set_show_close_button(true);
						window.queue_draw();
					}
				}
				return Ok(Some(gtk::Inhibit(false)));
			},
		});

		Ok(None)
	}
}

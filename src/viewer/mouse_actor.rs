/*!
 * Actor that takes care of hiding the mouse cursor when idle
 */

use super::*;
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use libhandy::prelude::*;

use actix::{Actor, AsyncContext};

pub fn create(builder: &woab::BuilderConnector) -> actix::Addr<HideMouseActor> {
	HideMouseActor::create(move |_ctx| HideMouseActor {
		timer: None,
		widgets: builder.widgets().unwrap(),
	})
}

pub struct HideMouseActor {
	timer: Option<tokio::task::JoinHandle<()>>,
	widgets: HideMouseActorWidgets,
}

#[derive(woab::WidgetsFromBuilder)]
pub struct HideMouseActorWidgets {
	window: gtk::Window,
	carousel_events: gtk::EventBox,
}

impl actix::Actor for HideMouseActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, _ctx: &mut Self::Context) {
		self.widgets.carousel_events.add_events(
			gdk::EventMask::STRUCTURE_MASK
				| gdk::EventMask::POINTER_MOTION_MASK
				| gdk::EventMask::ENTER_NOTIFY_MASK
				| gdk::EventMask::LEAVE_NOTIFY_MASK,
		);
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("HideMouseActor Quit");
	}
}

impl HideMouseActor {
	fn stop_timer(&mut self) {
		self.widgets.window.window().unwrap().set_cursor(None);
		if let Some(timer) = self.timer.take() {
			timer.abort();
		}
	}

	fn restart_timer(&mut self) {
		self.stop_timer();

		let window = self.widgets.window.window().unwrap();

		let until = actix::clock::Instant::now() + std::time::Duration::from_secs(4);
		self.timer = Some(actix::spawn(async move {
			actix::clock::sleep_until(until).await;
			woab::spawn_outside(async move {
				window.set_cursor(Some(&gdk::Cursor::for_display(
					&gdk::Display::default().unwrap(),
					gdk::CursorType::BlankCursor,
				)));
			});
		}));
	}
}

impl actix::Handler<woab::Signal> for HideMouseActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, _ctx: &mut Self::Context) -> woab::SignalResult {
		signal!(match (signal) {
			"on_motion" => |_, _event = gdk::Event| {
				self.restart_timer();
			},
			"on_enter" => |_, _event = gdk::Event| {
				self.restart_timer();
			},
			"on_leave" => |_, _event = gdk::Event| {
				self.stop_timer();
			},
		});

		Ok(Some(gtk::Inhibit(false)))
	}
}

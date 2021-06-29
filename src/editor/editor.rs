use actix::prelude::*;
use glib::clone;
use gtk::{cairo, gdk, glib, prelude::*};

use std::{cell::RefCell, rc::Rc};

use super::*;

pub(super) struct EditorActor {
	widgets: EditorWidgets,

	app: actix::Addr<AppActor>,
	current_page: Option<(Rc<RawPageImage>, Vec<Staff>, usize)>,
	selected_staff: Option<usize>,
	// Internal image cache
	editor_content: Rc<RefCell<(cairo::ImageSurface, bool)>>,
}

#[derive(woab::WidgetsFromBuilder)]
pub struct EditorWidgets {
	editor: gtk::DrawingArea,
}

impl actix::Actor for EditorActor {
	type Context = actix::Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let editor = &self.widgets.editor;
		editor.set_focus_on_click(true);
		editor.set_can_focus(true);
		editor.add_events(
			gdk::EventMask::POINTER_MOTION_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::BUTTON_RELEASE_MASK
				| gdk::EventMask::KEY_PRESS_MASK
				| gdk::EventMask::KEY_RELEASE_MASK,
		);

		{
			/* DrawingArea rendering */
			let addr = ctx.address();
			editor.connect_draw(clone!(@weak self.editor_content as editor_content => @default-panic, move |editor, context| {
				let (surface, _is_valid) = &mut *editor_content.borrow_mut();

				let (source_width, source_height) = (surface.width(), surface.height());
				let (target_width, target_height) = (editor.allocated_width(), editor.allocated_height());
				if (target_width, target_height) == (source_width, source_height) {
					context.set_source_surface(&surface, 0.0, 0.0);
					context.paint();
				} else {
					// TODO replace with optional
					if source_width > 0 && source_height > 0 {
						context.scale(target_width as f64 / source_width as f64, target_height as f64 / source_height as f64);
						context.set_source_surface(&surface, 0.0, 0.0);
						context.paint();
					}

					log::debug!("Queuing surface redraw");
					// tx.clone().try_send(EditorSignal::Redraw).unwrap();
					addr.try_send(EditorSignal2::Redraw).unwrap();
				}
				gtk::Inhibit::default()
			}));
		}
	}

	fn stopped(&mut self, _ctx: &mut Self::Context) {
		log::debug!("Editor Quit");
	}
}

impl actix::Handler<woab::Signal> for EditorActor {
	type Result = woab::SignalResult;

	fn handle(&mut self, signal: woab::Signal, _ctx: &mut Self::Context) -> woab::SignalResult {
		log::debug!("Editor signal: {:?}", signal.name());
		signal!(match (signal) {
			"button_press" => |editor = gtk::DrawingArea, event = gdk::Event| {
				let event: gdk::EventButton = event.downcast().unwrap();
				editor.emit_grab_focus();

				let (page, bars, _staves_before) = match &self.current_page {
					Some(current_page) => current_page,
					None => return Ok(Some(Inhibit(false))),
				};

				let scale = editor.allocated_height() as f64 / page.get_height();
				let x = event.position().0 / scale;
				let y = event.position().1 / scale;
				let mut selected_staff = None;
				for (i, bar) in bars.iter().enumerate() {
					if x > bar.left() && x < bar.right() && y > bar.top() && y < bar.bottom() {
						selected_staff = Some(i);
						break;
					}
				}
				if selected_staff != self.selected_staff {
					self.selected_staff = selected_staff;
					self.render_page();
					self.app.try_send(StaffSelected(selected_staff)).unwrap();
				}
			},
			"key_press" => |_editor = gtk::DrawingArea, event = gdk::Event| {
				let event: gdk::EventKey = event.downcast().unwrap();
				if event.keyval() == gdk::keys::constants::Delete
				|| event.keyval() == gdk::keys::constants::KP_Delete {
					self.selected_staff = None;
					self.render_page();
					self.app.try_send(DeleteSelectedStaff).unwrap();
				}
			},
			_ => | | {},
		});
		Ok(Some(Inhibit(false)))
	}
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub enum EditorSignal2 {
	Redraw,
	LoadPage(fragile::Fragile<Option<(Rc<RawPageImage>, Vec<Staff>, usize)>>),
}

impl actix::Handler<EditorSignal2> for EditorActor {
	type Result = ();

	fn handle(&mut self, signal: EditorSignal2, ctx: &mut Self::Context) {
		match signal {
			EditorSignal2::Redraw => self.render_page(),
			EditorSignal2::LoadPage(current_page) => {
				self.current_page = current_page.into_inner();
				self.selected_staff = None;
				self.app.try_send(StaffSelected(None)).unwrap();
				self.render_page()
			},
		}
		.expect("Failed to render page")
	}
}

impl EditorActor {
	pub fn new(app: actix::Addr<AppActor>, widgets: EditorWidgets) -> Self {
		EditorActor {
			widgets,
			app,

			editor_content: Rc::new(RefCell::new((
				cairo::ImageSurface::create(cairo::Format::Rgb24, 0, 0).unwrap(),
				false,
			))),
			current_page: None,
			selected_staff: None,
		}
	}

	fn render_page(&self) -> cair::Result<()> {
		let editor = &self.widgets.editor;
		editor.queue_draw();
		let (surface, _is_valid) = &mut *self.editor_content.borrow_mut();
		*surface = cairo::ImageSurface::create(
			cairo::Format::Rgb24,
			editor.allocated_width(),
			editor.allocated_height(),
		)
		.unwrap();
		let context = cairo::Context::new(&surface)?;
		context.set_source_rgb(1.0, 1.0, 1.0);
		context.paint()?;

		let (page, bars, staff_index_offset) = match &self.current_page {
			Some(selected_page) => selected_page,
			None => return Ok(()),
		};
		log::debug!("Drawing");
		// let staves_before: usize = self.staves_before;//pages.borrow().pages[0..selection as usize].iter().map(|(p, b)| b.len()).sum();
		// let page = &self.pages[selected_page.0];
		// let bars = state.staves;
		// let bars = &page.1;
		// let page = &page.0;

		let scale = editor.allocated_height() as f64 / page.get_height();
		context.scale(scale, scale);
		page.render(&context);

		context.save();
		context.set_source_rgba(0.1, 0.2, 0.4, 0.3);
		for (i, staff) in bars.iter().enumerate() {
			if Some(i) == self.selected_staff {
				/* Draw focused */
				context.save();

				/* Main shape */
				context.set_source_rgba(0.15, 0.3, 0.5, 0.3);
				context.rectangle(staff.left(), staff.top(), staff.width(), staff.height());
				context.fill_preserve();
				context.stroke();

				/* White handles on the corners */
				context.set_source_rgba(0.35, 0.6, 0.8, 1.0);
				context.arc(
					staff.left(),
					staff.top(),
					10.0,
					0.0,
					2.0 * std::f64::consts::PI,
				);
				context.fill()?;
				context.arc(
					staff.right(),
					staff.top(),
					10.0,
					0.0,
					2.0 * std::f64::consts::PI,
				);
				context.fill()?;
				context.arc(
					staff.left(),
					staff.bottom(),
					10.0,
					0.0,
					2.0 * std::f64::consts::PI,
				);
				context.fill()?;
				context.arc(
					staff.right(),
					staff.bottom(),
					10.0,
					0.0,
					2.0 * std::f64::consts::PI,
				);
				context.fill()?;

				context.restore();
			} else {
				context.rectangle(staff.left(), staff.top(), staff.width(), staff.height());
				context.fill_preserve();
				context.stroke();
			}
			context.save();
			context.set_font_size(25.0);
			context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
			context.move_to(staff.left() + 5.0, staff.bottom() - 5.0);
			context.show_text(&(staff_index_offset + i).to_string());
			context.restore();
		}
		context.restore();

		surface.flush();
		Ok(())
	}
}

use crate::EditorSongFile;
use dinoscore::{collection::*, prelude::*, *};

use std::sync::mpsc::*;

glib::wrapper! {
	pub struct EditorPage(ObjectSubclass<imp::EditorPage>)
		@extends gtk::Box, gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl EditorPage {
	pub fn init(&self, file: Rc<RefCell<EditorSongFile>>) {
		self.imp()
			.file
			.set(file)
			/* Because unwrap requires the type to be `Debug` :( */
			.map_err(|_| anyhow::format_err!("Internal error"))
			.unwrap();
	}

	pub fn load_page(&self, current_page: Option<PageIndex>) {
		self.imp().load_page(current_page);
	}

	pub fn update_page(&self) {
		self.imp().update_page();
	}

	#[cfg(test)]
	pub fn select_staff(&self, selected_staff: usize) {
		self.imp()
			.current_page
			.borrow_mut()
			.as_mut()
			.unwrap()
			.selected_staff = Some(selected_staff);
		self.imp().editor.queue_draw();
		self.imp().update_page_state();
	}
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Default)]
	#[template(resource = "/de/piegames/dinoscore/editor/page.ui")]
	pub struct EditorPage {
		#[template_child]
		pub editor: TemplateChild<gtk::DrawingArea>,
		#[template_child]
		piece_start: TemplateChild<gtk::CheckButton>,
		#[template_child]
		piece_name: TemplateChild<gtk::Entry>,
		#[template_child]
		section_start: TemplateChild<gtk::CheckButton>,
		#[template_child]
		section_repetition: TemplateChild<gtk::CheckButton>,
		#[template_child]
		section_end: TemplateChild<gtk::CheckButton>,

		pub(super) current_page: RefCell<Option<PageState>>,

		pub file: OnceCell<Rc<RefCell<EditorSongFile>>>,

		#[template_child]
		drag_gesture: TemplateChild<gtk::GestureDrag>,
		drag_state: Cell<Option<DragState>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for EditorPage {
		const NAME: &'static str = "EditorPage";
		type Type = super::EditorPage;
		type ParentType = gtk::Box;

		fn class_init(klass: &mut Self::Class) {
			klass.bind_template();
			klass.bind_template_callbacks();
		}

		fn instance_init(obj: &InitializingObject<Self>) {
			obj.init_template();
		}
	}

	impl ObjectImpl for EditorPage {
		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);
			self.editor.set_draw_func(clone!(@weak obj => @default-panic, move |editor, ctx, w, h| obj.imp().editor_draw(editor, ctx, w, h)));
		}
	}

	impl WidgetImpl for EditorPage {}

	impl BoxImpl for EditorPage {}

	#[gtk::template_callbacks]
	impl EditorPage {
		/// The content has changed externally
		pub fn update_page(&self) {
			let file = self.file.get().unwrap().borrow();
			if let Some(page) = self.current_page.borrow_mut().as_mut() {
				let bars = file.get_page(page.page).1;

				page.bars = bars;
				page.staves_before = file.count_staves_before(page.page);
			};
			self.editor.queue_draw();
			std::mem::drop(file);
			self.update_page_state();
		}

		/// The page selection has changed
		pub fn load_page(&self, current_page: Option<PageIndex>) {
			let file = self.file.get().unwrap().borrow();
			*self.current_page.borrow_mut() = current_page.map(|page_index| {
				let (image, bars) = file.get_page(page_index);
				let (renderer, update_page) = spawn_song_renderer(page_index, image.clone());

				update_page.attach(
					None,
					clone_!(self, move |obj, (image, index)| {
						if let Some(page) = obj.imp().current_page.borrow_mut().as_mut() {
							if index == page.page {
								page.image = Some(image);
							}
						}
						obj.imp().editor.queue_draw();
						Continue(true)
					}),
				);

				renderer.send(self.editor.get().width()).unwrap();

				PageState {
					page: page_index,
					image: None,
					bars,
					staves_before: file.count_staves_before(page_index),
					selected_staff: None,
					renderer,
				}
			});
			self.editor.queue_draw();
			std::mem::drop(file);
			self.update_page_state();
		}

		/// The page state has changed, update our widgets
		pub fn update_page_state(&self) {
			let file = self.file.get().unwrap().borrow();

			/* Absolute index of the currently selected staff, if presennt */
			let index: Option<usize> = self
				.current_page
				.borrow()
				.as_ref()
				.and_then(PageState::current_index);

			/* Set the selection */
			let piece_start_active = index
				.and_then(|i| file.piece_start(StaffIndex(i)))
				.is_some();

			let piece_name: String = index
				.and_then(|i: usize| file.piece_start(StaffIndex(i)))
				.cloned()
				.unwrap_or_default();
			let section_start: Option<SectionMeta> =
				index.and_then(|i| file.section_start(i.into()));
			let section_has_repetition = section_start
				.as_ref()
				.map(|meta| meta.is_repetition)
				.unwrap_or(false);
			let has_section_start = section_start.is_some();
			let has_section_end = section_start.map(|meta| meta.section_end).unwrap_or(false);

			/* Set the selected_staff to None to implicitly inhibit events */
			let selected_staff_backup = self
				.current_page
				.borrow_mut()
				.as_mut()
				.and_then(|page| page.selected_staff.take());

			/* In Swing/JavaFX, "active"=>"selected", "sensitive"=>"enabled"/"not disabled" */

			/* Disable the check box for the first item (it's force selected there) */
			let piece_start_sensitive = index.map(|i| i > 0).unwrap_or(false);
			self.piece_start.set_sensitive(piece_start_sensitive);
			self.piece_start.set_active(piece_start_active);

			self.piece_name.set_text(&piece_name);
			/* You can only enter a name on piece starts */
			self.piece_name.set_sensitive(piece_start_active);
			/* When a piece starts, a section must start as well, so it can't be edited */
			self.section_start
				.set_sensitive(!piece_start_active && piece_start_sensitive);
			self.section_start.set_active(has_section_start);

			self.section_repetition.set_sensitive(has_section_start);
			self.section_repetition.set_active(section_has_repetition);
			self.section_end.set_sensitive(has_section_start);
			self.section_end.set_active(has_section_end);

			if selected_staff_backup.is_some() {
				self.current_page
					.borrow_mut()
					.as_mut()
					.unwrap()
					.selected_staff = selected_staff_backup;
			}
			self.editor.queue_draw();
		}

		/// The drawingarea got clicked
		#[template_callback]
		fn on_resize(&self, width: i32, _height: i32) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			page.renderer.send(width).unwrap();
		}

		/// The drawingarea got clicked
		#[template_callback]
		fn on_click(&self, _n_press: i32, x: f64, y: f64) {
			self.editor.grab_focus();

			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let image = match page.image.as_ref() {
				Some(image) => image,
				None => return,
			};

			let scale =
				self.editor.get().height() as f64 / image.height() as f64 * image.width() as f64;
			let cast_result = page
				.cast_ray(x, y, scale)
				.map(|(i, _)| i)
				.collect::<Vec<_>>();

			if cast_result
				.iter()
				.find(|index| Some(**index) == page.selected_staff)
				.is_some()
			{
				/* Don't change selection if it is part of the result to avoid glitches */
				std::mem::drop(page_);
			} else if let Some(&selected_staff) = cast_result.iter().next() {
				page.selected_staff = Some(selected_staff);
				self.editor.queue_draw();
				std::mem::drop(page_);
				self.update_page_state();
			} else {
				page.selected_staff = None;
				std::mem::drop(page_);
				self.update_page_state();
			}
			self.on_motion(x, y);
		}

		#[template_callback]
		fn on_drag_start(&self, x: f64, y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let image = match page.image.as_ref() {
				Some(image) => image,
				None => return,
			};

			let scale =
				self.editor.get().height() as f64 / image.height() as f64 * image.width() as f64;
			let cast_result = page.cast_ray(x, y, scale).collect::<Vec<_>>();
			if let Some((index, handle)) = cast_result
				.iter()
				.find(|(index, _)| Some(*index) == page.selected_staff)
				.copied()
			{
				match handle {
					StaffHandle::Center => {
						self.drag_state.set(Some(DragState::Move(index)));
					},
					StaffHandle::Corner(dir) => {
						self.drag_state.set(Some(DragState::DragCorner(index, dir)));
					},
					StaffHandle::Edge(dir) => {
						self.drag_state.set(Some(DragState::DragEdge(index, dir)));
					},
				}
				page.selected_staff = Some(index);
			} else if cast_result.is_empty() {
				self.drag_state.set(Some(DragState::New));
			}
		}

		#[template_callback]
		fn on_drag_end(&self, _x: f64, _y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let image = match page.image.as_ref() {
				Some(image) => image,
				None => return,
			};
			let scale = self.editor.get().height() as f64 / image.height() as f64;
			let (mut x, mut y) = self.drag_gesture.start_point().unwrap();
			let (mut w, mut h) = self.drag_gesture.offset().unwrap();
			x /= scale;
			y /= scale;
			w /= scale;
			h /= scale;
			match self.drag_state.get() {
				Some(DragState::New) => {
					if w.abs() > 20.0 && h.abs() > 20.0 {
						let index = self.file.get().unwrap().borrow_mut().add_staff(
							page.page,
							Staff {
								page: page.page,
								start: (
									x.min(x + w) / image.width() as f64,
									y.min(y + h) / image.width() as f64,
								),
								end: (
									x.max(x + w) / image.width() as f64,
									y.max(y + h) / image.width() as f64,
								),
							},
						);
						page.selected_staff = Some(index);
						std::mem::drop(page_);
						self.update_page();
					} else {
						self.editor.queue_draw();
					}
				},
				Some(DragState::Move(index)) => {
					let index = self.file.get().unwrap().borrow_mut().move_staff(
						page.page,
						index,
						w / image.width() as f64,
						h / image.width() as f64,
					);
					page.selected_staff = Some(index);
					std::mem::drop(page_);
					self.update_page();
				},
				Some(DragState::DragCorner(index, dir) | DragState::DragEdge(index, dir)) => {
					let index = self.file.get().unwrap().borrow_mut().modify_staff(
						page.page,
						index,
						|staff| {
							let dx = w / image.width() as f64;
							let dy = h / image.width() as f64;

							if dir.contains('n') {
								staff.start.1 += dy;
							}
							if dir.contains('e') {
								staff.end.0 += dx;
							}
							if dir.contains('s') {
								staff.end.1 += dy;
							}
							if dir.contains('w') {
								staff.start.0 += dx;
							}
							/* Fixups */
							if staff.start.0 > staff.end.0 {
								std::mem::swap(&mut staff.start.0, &mut staff.end.0);
							}
							if staff.start.1 > staff.end.1 {
								std::mem::swap(&mut staff.start.1, &mut staff.end.1);
							}
						},
					);
					page.selected_staff = Some(index);
					std::mem::drop(page_);
					self.update_page();
				},
				None => {
					self.editor.queue_draw();
				},
			}
			self.drag_state.set(None);
		}

		#[template_callback]
		fn on_drag_update(&self, _x: f64, _y: f64) {
			self.editor.queue_draw();
		}

		/// Key press on the drawingarea
		#[template_callback]
		fn on_key(&self, keyval: gdk::Key) {
			if keyval == gdk::Key::Delete || keyval == gdk::Key::KP_Delete {
				self.delete_selected_staff();
			}
		}

		#[template_callback]
		fn on_leave(&self, _controller: gtk::EventControllerMotion) {
			// log::debug!("Cursor: none");
			self.instance().set_cursor(None);
		}

		#[template_callback]
		fn on_motion(&self, x: f64, y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let image = match page.image.as_ref() {
				Some(image) => image,
				None => return,
			};

			if self.drag_state.get().is_none() {
				let scale = self.editor.get().height() as f64 / image.height() as f64
					* image.width() as f64;
				let cast_result = page.cast_ray(x, y, scale).collect::<Vec<_>>();

				if let Some((_, handle)) = cast_result
					.iter()
					.find(|(index, _)| Some(*index) == page.selected_staff)
				{
					match handle {
						StaffHandle::Center => {
							// log::debug!("Cursor: move");
							self.instance().set_cursor_from_name(Some("move"));
						},
						StaffHandle::Corner(dir) | StaffHandle::Edge(dir) => {
							// log::debug!("Cursor: {dir}");
							self.instance()
								.set_cursor_from_name(Some(&format!("{dir}-resize")));
						},
					}
				} else if !cast_result.is_empty() {
					// log::debug!("Cursor: none");
					self.instance().set_cursor(None);
				} else {
					// log::debug!("Cursor: cell");
					self.instance().set_cursor_from_name(Some("cell"));
				}
			}
		}

		fn delete_selected_staff(&self) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff.take() {
				Some(selected_staff) => selected_staff,
				None => return,
			};

			let mut file = self.file.get().unwrap().borrow_mut();
			file.delete_staff(page.page, selected_staff);

			std::mem::drop((page_, file));
			self.update_page();
		}

		#[template_callback]
		fn update_part_name(&self) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff {
				Some(selected_staff) => selected_staff,
				None => return,
			};
			let mut file = self.file.get().unwrap().borrow_mut();
			let index = StaffIndex(file.count_staves_before(page.page) + selected_staff);
			let name = file
				.piece_start_mut(index)
				.as_mut()
				.expect("You shouldn't be able to set the name on non part starts");
			*name = self.piece_name.text().to_string();
		}

		#[template_callback]
		fn update_section_start(&self) {
			let selected = self.section_start.is_active();
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff {
				Some(selected_staff) => selected_staff,
				None => return,
			};
			let mut file = self.file.get().unwrap().borrow_mut();
			let index = StaffIndex(file.count_staves_before(page.page) + selected_staff);
			if selected {
				file.section_start_mut(index)
					.get_or_insert_with(SectionMeta::default);
			} else {
				file.section_start_mut(index).take();
			}

			std::mem::drop((page_, file));
			self.update_page_state();
		}

		#[template_callback]
		fn update_section_repetition(&self) {
			let selected = self.section_repetition.is_active();
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff {
				Some(selected_staff) => selected_staff,
				None => return,
			};
			let mut file = self.file.get().unwrap().borrow_mut();
			let index = StaffIndex(file.count_staves_before(page.page) + selected_staff);
			file.section_start_mut(index)
				.as_mut()
				.expect("You shouldn't be able to click this if there's no section start")
				.is_repetition = selected;

			std::mem::drop((page_, file));
			self.update_page_state();
		}

		#[template_callback]
		fn update_section_end(&self) {
			let selected = self.section_end.is_active();
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff {
				Some(selected_staff) => selected_staff,
				None => return,
			};
			let mut file = self.file.get().unwrap().borrow_mut();
			let index = StaffIndex(file.count_staves_before(page.page) + selected_staff);
			file.section_start_mut(index)
				.as_mut()
				.expect("You shouldn't be able to click this if there's no section start")
				.section_end = selected;

			std::mem::drop((page_, file));
			self.update_page_state();
		}

		#[template_callback]
		fn update_part_start(&self) {
			let selected = self.piece_start.is_active();
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let selected_staff = match page.selected_staff {
				Some(selected_staff) => selected_staff,
				None => return,
			};
			let mut file = self.file.get().unwrap().borrow_mut();
			let index = StaffIndex(file.count_staves_before(page.page) + selected_staff);
			if selected {
				file.piece_start_mut(index)
					.get_or_insert_with(Default::default);
				/* When a piece starts, a section must start as well */
				file.section_start_mut(index)
					.get_or_insert_with(SectionMeta::default);
			} else {
				file.piece_start_mut(index).take();
			}

			std::mem::drop((page_, file));
			self.update_page_state();
		}

		/// Draw signal
		fn editor_draw(
			&self,
			editor: &gtk::DrawingArea,
			context: &cairo::Context,
			_width: i32,
			_height: i32,
		) {
			let mut page = self.current_page.borrow_mut();
			let page = match page.as_mut() {
				Some(page) => page,
				None => return,
			};
			let image = match page.image.as_ref() {
				Some(image) => image,
				None => return, // TODO draw gray area or something
			};
			let file = self.file.get().unwrap().borrow();
			if editor.height() < 1 {
				return;
			}
			catch!({
				context.set_source_rgb(1.0, 1.0, 1.0);
				context.paint()?;

				let scale = editor.height() as f64 / image.height() as f64;
				context.save()?;
				context.scale(scale, scale);
				context.set_source_pixbuf(&gdk::pixbuf_get_from_texture(image).unwrap(), 0.0, 0.0);
				context.paint()?;
				context.restore()?;

				let effective_image_width = image.width() as f64 * scale;

				context.save()?;
				context.set_source_rgba(0.1, 0.2, 0.4, 0.3);
				if let Some(DragState::New) = self.drag_state.get() {
					let (x, y) = self.drag_gesture.start_point().unwrap();
					let (w, h) = self.drag_gesture.offset().unwrap();
					context.rectangle(x, y, w, h);
					context.fill_preserve()?;
					context.stroke()?;
				}
				for (i, staff) in page.bars.iter().enumerate() {
					let absolute_index = page.staves_before + i;
					let section_number = file.count_sections_until(StaffIndex(absolute_index));

					context.save()?;
					match section_number % 4 {
						0 => context.set_source_rgba(0.2, 0.1, 0.4, 0.3),
						1 => context.set_source_rgba(0.1, 0.4, 0.2, 0.3),
						2 => context.set_source_rgba(0.4, 0.2, 0.1, 0.3),
						3 => context.set_source_rgba(0.4, 0.1, 0.2, 0.3),
						_ => unreachable!(),
					}

					/* Transform coordinates */
					let mut staff_left = staff.left() * effective_image_width;
					let mut staff_right = staff.right() * effective_image_width;
					let mut staff_top = staff.top() * effective_image_width;
					let mut staff_bottom = staff.bottom() * effective_image_width;

					match self.drag_state.get() {
						Some(DragState::Move(index)) if index == i => {
							let (dx, dy) = self.drag_gesture.offset().unwrap();
							context.translate(dx, dy);
						},
						Some(
							DragState::DragCorner(index, dir) | DragState::DragEdge(index, dir),
						) if index == i => {
							let (dx, dy) = self.drag_gesture.offset().unwrap();
							if dir.contains('n') {
								staff_top += dy;
							}
							if dir.contains('e') {
								staff_right += dx;
							}
							if dir.contains('s') {
								staff_bottom += dy;
							}
							if dir.contains('w') {
								staff_left += dx;
							}
							/* Fixups */
							if staff_left > staff_right {
								std::mem::swap(&mut staff_left, &mut staff_right);
							}
							if staff_top > staff_bottom {
								std::mem::swap(&mut staff_top, &mut staff_bottom);
							}
						},
						_ => {},
					}

					let staff_width = staff_right - staff_left;
					let staff_height = staff_bottom - staff_top;

					if Some(i) == page.selected_staff {
						/* Draw focused */
						context.save()?;

						/* Main shape */
						context.set_source_rgba(0.15, 0.3, 0.5, 0.3);
						context.rectangle(staff_left, staff_top, staff_width, staff_height);
						context.fill_preserve()?;
						context.stroke()?;

						/* White handles on the corners */
						context.set_source_rgba(0.35, 0.6, 0.8, 1.0);
						context.arc(staff_left, staff_top, 10.0, 0.0, 2.0 * std::f64::consts::PI);
						context.fill()?;
						context.arc(
							staff_right,
							staff_top,
							10.0,
							0.0,
							2.0 * std::f64::consts::PI,
						);
						context.fill()?;
						context.arc(
							staff_left,
							staff_bottom,
							10.0,
							0.0,
							2.0 * std::f64::consts::PI,
						);
						context.fill()?;
						context.arc(
							staff_right,
							staff_bottom,
							10.0,
							0.0,
							2.0 * std::f64::consts::PI,
						);
						context.fill()?;

						context.restore()?;
					} else {
						context.rectangle(staff_left, staff_top, staff_width, staff_height);
						context.fill_preserve()?;
						context.stroke()?;
					}
					context.save()?;
					context.set_font_size(25.0);
					context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
					context.move_to(staff_left + 5.0, staff_bottom - 5.0);
					context.show_text(&absolute_index.to_string())?;
					context.restore()?;
					context.restore()?;
				}
				context.restore()?;
				cairo::Result::Ok(())
			})
			.expect("Rendering failed");
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragState {
	Move(usize),
	New,
	/* The string is one of ne, nw, sw, se */
	DragCorner(usize, &'static str),
	/* The string is one of n, e, s, w */
	DragEdge(usize, &'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaffHandle {
	/* The string is one of ne, nw, sw, se */
	Corner(&'static str),
	/* The string is one of n, e, s, w */
	Edge(&'static str),
	Center,
}

/// When a page is loaded, store our state in here
struct PageState {
	page: PageIndex,
	image: Option<gdk::Texture>,
	/// Only those from the current page
	bars: Vec<Staff>,
	staves_before: usize,
	// selected_page: PageIndex,
	/* Relative to the currently selected page */
	selected_staff: Option<usize>,
	/// Send new canvas size to background thread
	renderer: Sender<i32>,
}

impl PageState {
	/// Absolute staff index
	fn current_index(&self) -> Option<usize> {
		self.selected_staff.map(|s| self.staves_before + s)
	}

	/// If we hit, return staff index of page and hit kind
	fn cast_ray(
		&self,
		x: f64,
		y: f64,
		scale: f64,
	) -> impl Iterator<Item = (usize, StaffHandle)> + '_ {
		let x = x / scale;
		let y = y / scale;
		let radius = 10.0 / scale;
		let edge_width = radius / 2.0;
		self.bars.iter().enumerate().filter_map(move |(i, bar)| {
			/* Check for corners first */
			for (dir, corner_x, corner_y) in [
				("nw", bar.left(), bar.top()),
				("sw", bar.left(), bar.bottom()),
				("ne", bar.right(), bar.top()),
				("se", bar.right(), bar.bottom()),
			] {
				if f64::sqrt((corner_x - x) * (corner_x - x) + (corner_y - y) * (corner_y - y))
					< radius
				{
					return Some((i, StaffHandle::Corner(dir)));
				}
			}

			/* Then, check for edges */
			let w = edge_width;
			for (dir, l, r, t, b) in [
				("w", bar.left() - w, bar.left() + w, bar.top(), bar.bottom()),
				(
					"e",
					bar.right() - w,
					bar.right() + w,
					bar.top(),
					bar.bottom(),
				),
				("n", bar.left(), bar.right(), bar.top() - w, bar.top() + w),
				(
					"s",
					bar.left(),
					bar.right(),
					bar.bottom() - w,
					bar.bottom() + w,
				),
			] {
				if x >= l && x <= r && y >= t && y <= b {
					return Some((i, StaffHandle::Edge(dir)));
				}
			}

			/* Last, check for inner */
			if x > bar.left() && x < bar.right() && y > bar.top() && y < bar.bottom() {
				return Some((i, StaffHandle::Center));
			}

			None
		})
	}
}

/// A background thread renderer
///
/// It will take the raw PDFs and images and render them scaled down to an appropriate
/// size. It is flexible with in-flight requests and invalidation.
///
/// Drop one of the channels when you are no longer interested in that image.
///
/// This is as scoped down version of the background renderer in `song_widget.rs`
fn spawn_song_renderer(
	index: PageIndex,
	page: Arc<PageImage>,
) -> (Sender<i32>, glib::Receiver<(gdk::Texture, PageIndex)>) {
	let (in_tx, in_rx) = channel();
	let (out_tx, out_rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

	std::thread::spawn(move || {
		/* For a start, render at minimum resolution. This should not take long */
		let mut last_width = 250;
		let image = gdk::Texture::for_pixbuf(&page.render_scaled(250));
		if out_tx.send((image, index)).is_err() {
			return;
		}
		log::debug!("Background renderer ready");

		/* We always only want the latest value */
		fn fetch_latest(rx: &Receiver<i32>) -> Option<i32> {
			let mut last = None::<i32>;
			loop {
				match (rx.try_recv(), &mut last) {
					(Ok(val), last) => {
						*last = Some(val);
					},
					(Err(TryRecvError::Empty), None) => {
						/* Don't return empty handed */
						return rx.recv().ok();
					},
					(Err(TryRecvError::Empty), Some(last)) => return Some(*last),
					(Err(TryRecvError::Disconnected), _) => return None,
				}
			}
		}

		loop {
			match fetch_latest(&in_rx) {
				Some(requested_width) => {
					/* Round the width to the nearest level. Never round down, never round more than
					 * 66% (the levels are 2/3 apart each, exponentially). Never go below 250 pixels.
					 */
					let actual_width = (1.5f64)
						.powf((requested_width as f64).log(1.5).ceil())
						.ceil()
						.max(250.0) as i32;
					if actual_width != last_width {
						last_width = actual_width;
						log::debug!("Background thread rendering width changed: {actual_width}");

						let image = gdk::Texture::for_pixbuf(&page.render_scaled(actual_width));

						/* Send it off */
						if out_tx.send((image, index)).is_err() {
							return;
						}
					}
				},
				None => return,
			}
		}
	});

	(in_tx, out_rx)
}

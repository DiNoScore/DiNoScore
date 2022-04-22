use crate::EditorSongFile;
use dinoscore::{collection::*, prelude::*, *};

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
}

mod imp {
	use super::*;

	#[derive(CompositeTemplate, Default)]
	#[template(resource = "/de/piegames/dinoscore/editor/page.ui")]
	pub struct EditorPage {
		#[template_child]
		editor: TemplateChild<gtk::DrawingArea>,
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

		current_page: RefCell<Option<PageState>>,

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
				let (image, bars) = file.pages[page.page.0].clone();
				page.image = image;
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
				let (image, bars) = file.pages[page_index.0].clone();
				PageState {
					page: page_index,
					image,
					bars,
					staves_before: file.count_staves_before(page_index),
					selected_staff: None,
				}
			});
			self.editor.queue_draw();
			std::mem::drop(file);
			self.update_page_state();
		}

		/// The page state has changed, update our widgets
		fn update_page_state(&self) {
			let file = self.file.get().unwrap().borrow();

			/* Absolute index of the currently selected staff, if presennt */
			let index: Option<usize> = self
				.current_page
				.borrow()
				.as_ref()
				.and_then(PageState::current_index);

			/* Set the selection */
			let piece_start_active = index
				.and_then(|i| file.piece_starts.get(&StaffIndex(i)))
				.is_some();

			let piece_name: String = index
				.and_then(|i: usize| file.piece_starts.get(&StaffIndex(i)))
				.cloned()
				.unwrap_or_default();
			let section_start: Option<&SectionMeta> =
				index.and_then(|i| file.section_starts.get(&i.into()));
			let section_has_repetition = section_start
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
		}

		/// The drawingarea got clicked
		#[template_callback]
		fn on_click(&self, _n_press: i32, x_: f64, y_: f64) {
			self.editor.grab_focus();

			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};

			let scale = self.editor.get().height() as f64 / page.image.get_height();
			let x = x_ / scale;
			let y = y_ / scale;
			let selected_staff = page.cast_ray(x, y).map(|(i, _)| i);
			if selected_staff != page.selected_staff {
				page.selected_staff = selected_staff;
				self.editor.queue_draw();
				std::mem::drop(page_);
				self.update_page_state();
			} else {
				std::mem::drop(page_);
			}
			self.on_motion(x_, y_);
		}

		#[template_callback]
		fn on_drag_start(&self, x: f64, y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};

			match page.cast_ray(x, y) {
				Some((_index, StaffHandle::Corner)) => {
					// self.drag_state.set(true);
					todo!()
				},
				Some((_index, StaffHandle::Edge)) => todo!(),
				Some((index, StaffHandle::Center)) => {
					if Some(index) == page.selected_staff {
						self.drag_state.set(Some(DragState::Move(index)));
						self.instance().set_cursor_from_name(Some("move"));
					}
				},
				None => {
					self.drag_state.set(Some(DragState::New));
					self.instance().set_cursor_from_name(Some("cell"));
				},
			}
		}

		#[template_callback]
		fn on_drag_end(&self, _x: f64, _y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};
			let scale = self.editor.get().height() as f64 / page.image.get_height();
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
								start: (x.min(x + w), y.min(y + h)),
								end: (x.max(x + w), y.max(y + h)),
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
					let index = self
						.file
						.get()
						.unwrap()
						.borrow_mut()
						.move_staff(page.page, index, w, h);
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
			// self.instance().set_cursor(None);
		}

		#[template_callback]
		fn on_motion(&self, x: f64, y: f64) {
			let mut page_ = self.current_page.borrow_mut();
			let page = match page_.as_mut() {
				Some(page) => page,
				None => return,
			};

			if self.drag_state.get().is_some() {
				let scale = self.editor.get().height() as f64 / page.image.get_height();
				let x = x / scale;
				let y = y / scale;
				match page.cast_ray(x, y) {
					Some((index, StaffHandle::Center)) if Some(index) == page.selected_staff => {
						self.instance().set_cursor_from_name(Some("move"));
					},
					_ => {
						self.instance().set_cursor(None);
					},
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
				.piece_starts
				.get_mut(&index)
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
				file.section_starts
					.entry(index)
					.or_insert_with(SectionMeta::default);
			} else {
				file.section_starts.remove(&index);
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
			file.section_starts
				.get_mut(&index)
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
			file.section_starts
				.get_mut(&index)
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
				file.piece_starts.entry(index).or_insert_with(|| "".into());
				/* When a piece starts, a section must start as well */
				file.section_starts
					.entry(index)
					.or_insert_with(SectionMeta::default);
			} else {
				file.piece_starts.remove(&index);
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
			catch!({
				context.set_source_rgb(1.0, 1.0, 1.0);
				context.paint()?;

				let scale = editor.height() as f64 / page.image.get_height();
				context.scale(scale, scale);
				page.image.render(context)?;

				context.save()?;
				context.set_source_rgba(0.1, 0.2, 0.4, 0.3);
				if let Some(DragState::New) = self.drag_state.get() {
					let scale = self.editor.get().height() as f64 / page.image.get_height();
					let (mut x, mut y) = self.drag_gesture.start_point().unwrap();
					let (mut w, mut h) = self.drag_gesture.offset().unwrap();
					// TODO maybe apply some global transformation instead?
					x /= scale;
					y /= scale;
					w /= scale;
					h /= scale;
					context.rectangle(x, y, w, h);
					context.fill_preserve()?;
					context.stroke()?;
				}
				for (i, staff) in page.bars.iter().enumerate() {
					context.save()?;

					if let Some(DragState::Move(index)) = self.drag_state.get() {
						if index == i {
							let scale = self.editor.get().height() as f64 / page.image.get_height();
							let (w, h) = self.drag_gesture.offset().unwrap();
							context.translate(w / scale, h / scale);
						}
					}
					if Some(i) == page.selected_staff {
						/* Draw focused */
						context.save()?;

						/* Main shape */
						context.set_source_rgba(0.15, 0.3, 0.5, 0.3);
						context.rectangle(staff.left(), staff.top(), staff.width(), staff.height());
						context.fill_preserve()?;
						context.stroke()?;

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

						context.restore()?;
					} else {
						context.rectangle(staff.left(), staff.top(), staff.width(), staff.height());
						context.fill_preserve()?;
						context.stroke()?;
					}
					context.save()?;
					context.set_font_size(25.0);
					context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
					context.move_to(staff.left() + 5.0, staff.bottom() - 5.0);
					context.show_text(&(page.staves_before + i).to_string())?;
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
	// DragCorner {
	// 	staff: usize,
	// }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StaffHandle {
	Corner,
	Edge,
	Center,
}

/// When a page is loaded, store our state in here
struct PageState {
	page: PageIndex,
	image: Rc<RawPageImage>,
	/// Only those from the current page
	bars: Vec<Staff>,
	staves_before: usize,
	// selected_page: PageIndex,
	/* Relative to the currently selected page */
	selected_staff: Option<usize>,
}

impl PageState {
	/// Absolute staff index
	fn current_index(&self) -> Option<usize> {
		self.selected_staff.map(|s| self.staves_before + s)
	}

	/// If we hit, return staff index of page and hit kind
	fn cast_ray(&self, x: f64, y: f64) -> Option<(usize, StaffHandle)> {
		let radius = 10.0;
		for (i, bar) in self.bars.iter().enumerate() {
			/* Check for corners first */
			// for (corner_x, corner_y) in [
			// 	(bar.left(), bar.top()),
			// 	(bar.left(), bar.bottom()),
			// 	(bar.right(), bar.top()),
			// 	(bar.right(), bar.bottom()),
			// ] {
			// 	if f64::sqrt((corner_x - x) * (corner_x - x) + (corner_y - y) * (corner_y - y)) < radius {
			// 		return Some((i, StaffHandle::Corner))
			// 	}
			// }

			/* Then, check for edges */
			// TODO

			/* Last, check for inner */
			if x > bar.left() && x < bar.right() && y > bar.top() && y < bar.bottom() {
				return Some((i, StaffHandle::Center));
			}
		}

		None
	}
}

use blocking::unblock;
use dinoscore::song::*;
use futures::{executor::block_on, prelude::*};
use gdk::prelude::*;
use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;
use itertools::Itertools;
use maplit::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[macro_use]
extern crate serde;

struct EditorArea {
	editor: gtk::DrawingArea,
	state: Rc<RefCell<Option<EditorAreaState>>>,
}

struct EditorAreaState {
	selected_staff: Option<usize>,
	staves_before: usize,
	page: Rc<(poppler::PopplerPage, RefCell<Vec<Staff>>)>,
}

impl EditorAreaState {
	fn delete_staff(&mut self, index: usize) {
		self.selected_staff = None;
		self.page.1.borrow_mut().remove(index);
	}
}

impl EditorArea {
	fn new(builder: &gtk::Builder, editor_tx: glib::Sender<EditorViewUpdateEvent>) -> Self {
		let editor: gtk::DrawingArea = builder.get_object("editor").unwrap();
		let state = Rc::new(RefCell::new(Option::<EditorAreaState>::None));

		editor.set_focus_on_click(true);
		editor.set_can_focus(true);
		editor.add_events(
			gdk::EventMask::POINTER_MOTION_MASK
				| gdk::EventMask::BUTTON_PRESS_MASK
				| gdk::EventMask::BUTTON_RELEASE_MASK
				| gdk::EventMask::KEY_PRESS_MASK
				| gdk::EventMask::KEY_RELEASE_MASK,
		);
		editor.connect_key_press_event(clone!(@strong state, @strong editor_tx =>
			move |_editor, event| {
				if event.get_keyval() == gdk::keys::constants::Delete
				|| event.get_keyval() == gdk::keys::constants::KP_Delete {
					if let Some(state) = &mut *state.borrow_mut() {
						if let Some(index) = state.selected_staff {
							state.delete_staff(index as usize);
							editor_tx.send(EditorViewUpdateEvent::UpdateSelection { page_changed: false }).unwrap();
						}
					}
				}
				gtk::Inhibit::default()
			}
		));
		editor.connect_button_press_event(
			clone!(@strong state, @strong editor_tx => move |editor, event| {
				if let Some(state) = &mut *state.borrow_mut() {
					editor.emit_grab_focus();

					let page = &state.page.0;
					let bars = &state.page.1.borrow();

					let scale = editor.get_allocated_height() as f64 / page.get_size().1;
					let x = event.get_position().0 / scale / page.get_size().0;
					let y = event.get_position().1 / scale / page.get_size().1;
					let mut hit = false;
					for (i, bar) in bars.iter().enumerate() {
						if x > bar.left && x < bar.right && y > bar.top && y < bar.bottom {
							state.selected_staff = Some(i);
							hit = true;
							break;
						}
					}
					if !hit {
						state.selected_staff = None;
					}
					editor_tx.send(EditorViewUpdateEvent::UpdateSelection { page_changed: false }).unwrap();
				}
				gtk::Inhibit::default()
			}),
		);
		editor.connect_button_release_event(|_, event| {
			// dbg!(&event, event.get_position());
			gtk::Inhibit::default()
		});
		editor.connect_drag_begin(|_, context| {
			dbg!(&context);
		});
		editor.connect_drag_motion(|_, context, x, y, time| {
			dbg!(&context, x, y, time);
			gtk::Inhibit::default()
		});
		editor.connect_motion_notify_event(|_, event| {
			// dbg!(&event.get_state());
			gtk::Inhibit::default()
		});

		editor.connect_draw(clone!(@strong state => move |editor, context| {
			if let Some(state) = &*state.borrow() {
				let staves_before: usize = state.staves_before;//pages.borrow().pages[0..selection as usize].iter().map(|(p, b)| b.len()).sum();
				// let page = state.page;
				// let bars = state.staves;
				let page = &state.page.0;
				let bars = &state.page.1.borrow();

				let scale = editor.get_allocated_height() as f64 / page.get_size().1;
				context.scale(scale, scale);

				context.set_source_rgb(1.0, 1.0, 1.0);
				context.paint();
				page.render(&context);

				context.save();
				context.set_source_rgba(0.1, 0.2, 0.4, 0.3);
				for (i, staff) in bars.iter().enumerate() {
					if Some(i) == state.selected_staff {
						/* Draw focused */
						context.save();

						/* Main shape */
						context.set_source_rgba(0.15, 0.3, 0.5, 0.3);
						context.rectangle(
							staff.left*page.get_size().0,
							staff.top*page.get_size().1,
							(staff.right-staff.left)*page.get_size().0,
							(staff.bottom-staff.top)*page.get_size().1);
						context.fill_preserve();
						context.stroke();

						/* White handles on the corners */
						context.set_source_rgba(0.35, 0.6, 0.8, 1.0);
						context.arc(
							staff.left*page.get_size().0,
							staff.top*page.get_size().1,
							10.0, 0.0, 2.0 * std::f64::consts::PI);
						context.fill();
						context.arc(
							staff.right*page.get_size().0,
							staff.top*page.get_size().1,
							10.0, 0.0, 2.0 * std::f64::consts::PI);
						context.fill();
						context.arc(
							staff.left*page.get_size().0,
							staff.bottom*page.get_size().1,
							10.0, 0.0, 2.0 * std::f64::consts::PI);
						context.fill();
						context.arc(
							staff.right*page.get_size().0,
							staff.bottom*page.get_size().1,
							10.0, 0.0, 2.0 * std::f64::consts::PI);
						context.fill();

						context.restore();
					} else {
						context.rectangle(
							staff.left*page.get_size().0,
							staff.top*page.get_size().1,
							(staff.right-staff.left)*page.get_size().0,
							(staff.bottom-staff.top)*page.get_size().1);
						context.fill_preserve();
						context.stroke();
					}
					context.save();
					context.set_font_size(25.0);
					context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
					context.move_to(staff.left*page.get_size().0 + 5.0, staff.bottom*page.get_size().1 - 5.0);
					context.show_text(&(staves_before + i).to_string());
					context.restore();
				}
				context.restore();
			}
			gtk::Inhibit::default()
		}));

		EditorArea { editor, state }
	}

	fn unload_page(&mut self) {
		*self.state.borrow_mut() = None;
		self.editor.queue_draw();
	}

	fn load_page(
		&mut self,
		page: Rc<(poppler::PopplerPage, RefCell<Vec<Staff>>)>,
		staves_before: usize,
	) {
		*self.state.borrow_mut() = Some(EditorAreaState {
			selected_staff: None,
			page,
			staves_before,
		});
		self.editor.queue_draw();
	}
}

enum EditorViewUpdateEvent {
	Void,
	UpdateSelection { page_changed: bool },
	AppendPage(gdk_pixbuf::Pixbuf),
	RemovePage(PageIndex),
	Reload,
}

struct EditorView {
	area: EditorArea,
	piece_start: gtk::CheckButton,
	piece_name: gtk::Entry,
	section_start: gtk::CheckButton,
	section_repetition: gtk::CheckButton,
	section_end: gtk::CheckButton,
	pages_preview: gtk::IconView,
	/* Pixbufs preview cache */
	pages_preview_data: gtk::ListStore,
	pages_preview_callback: glib::SignalHandlerId,
	state: Rc<RefCell<EditorState>>,
}

impl EditorView {
	fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
		let (editor_tx, editor_rx) =
			glib::MainContext::channel::<EditorViewUpdateEvent>(glib::Priority::default());
		let area = EditorArea::new(&builder, editor_tx.clone());

		let add_pages: gtk::ToolButton = builder.get_object("add_pages").unwrap();
		let add_pages2: gtk::ToolButton = builder.get_object("add_pages2").unwrap();
		let autodetect: gtk::ToolButton = builder.get_object("autodetect").unwrap();
		let autodetect2: gtk::ToolButton = builder.get_object("autodetect2").unwrap();
		let pages_preview: gtk::IconView = builder.get_object("pages_preview").unwrap();
		let pages_preview_data: gtk::ListStore = builder.get_object("store_pages").unwrap();

		let piece_start: gtk::CheckButton = builder.get_object("piece_start").unwrap();
		let piece_name: gtk::Entry = builder.get_object("piece_name").unwrap();
		let section_start: gtk::CheckButton = builder.get_object("section_start").unwrap();
		let section_repetition: gtk::CheckButton =
			builder.get_object("section_repetition").unwrap();
		let section_end: gtk::CheckButton = builder.get_object("section_end").unwrap();

		let state = Rc::new(RefCell::new(EditorState::new(
			editor_tx,
			area.state.clone(),
		)));

		add_pages.connect_clicked(clone!(@strong state => @default-panic, move |_button| {
			let filter = gtk::FileFilter::new();
			filter.add_pixbuf_formats();
			filter.add_mime_type("application/pdf");
			let choose = gtk::FileChooserNativeBuilder::new()
				.title("Select images or PDFs to load")
				.action(gtk::FileChooserAction::Open)
				.select_multiple(true)
				.filter(&filter)
				.build();
			if choose.run() == gtk::ResponseType::Accept {
				for file in choose.get_files() {
					let path = file.get_path().unwrap();
					let pdf = if let Some("pdf") = path.as_path().extension().and_then(std::ffi::OsStr::to_str) {
						poppler::PopplerDocument::new_from_file(path, "").unwrap()
					} else {
						pixbuf_to_pdf(&gdk_pixbuf::Pixbuf::from_file(&path).unwrap())
					};
					for page in 0..pdf.get_n_pages() {
						let page = pdf.get_page(page).unwrap();
						state.borrow_mut().add_page(page);
					}
				}
			}
		}));

		add_pages2.connect_clicked(clone!(@strong state => @default-panic, move |_button| {
			let filter = gtk::FileFilter::new();
			filter.add_pixbuf_formats();
			let choose = gtk::FileChooserNativeBuilder::new()
				.title("Select images to load")
				.action(gtk::FileChooserAction::Open)
				.select_multiple(true)
				.filter(&filter)
				.build();
			if choose.run() == gtk::ResponseType::Accept {
				for file in choose.get_files() {
					let path = file.get_path().unwrap();
					let image = opencv::imgcodecs::imread(&path.to_str().unwrap(), 0).unwrap();
					
					let mut image_binarized = opencv::core::Mat::default().unwrap();
					opencv::imgproc::adaptive_threshold(&image, &mut image_binarized, 255.0, 
						opencv::imgproc::AdaptiveThresholdTypes::ADAPTIVE_THRESH_MEAN_C as i32,
						opencv::imgproc::ThresholdTypes::THRESH_BINARY as i32,
						101, 30.0
					).unwrap();
					
					let mut image_binarized_median = opencv::core::Mat::default().unwrap();
					opencv::imgproc::median_blur(&image_binarized, &mut image_binarized_median, 3).unwrap();
	
					dbg!(opencv::imgcodecs::imwrite("./tmp.png", &image_binarized_median, &opencv::core::Vector::new()).unwrap());
					/* The easiest way to convert Mat to Pixbuf is to write it to a PNG buffer */
					let mut png = opencv::core::Vector::new();
					dbg!(opencv::imgcodecs::imencode(
						".png",
						&image_binarized_median,
						&mut png,
						&opencv::core::Vector::new(),
					).unwrap());
					let pixbuf = gdk_pixbuf::Pixbuf::from_stream(
						/* How many type conversion layers will we pile today? */
						&gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&png.to_vec())),
						Option::<&gio::Cancellable>::None,
					).unwrap();
					let pdf = pixbuf_to_pdf(&pixbuf);
					for page in 0..pdf.get_n_pages() {
						let page = pdf.get_page(page).unwrap();
						state.borrow_mut().add_page(page);
					}
				}
			}
		}));

		autodetect.connect_clicked(clone!(@strong state, @weak pages_preview, @weak pages_preview_data => @default-panic, move |_button| {
			let state = state.clone();
			glib::MainContext::default().spawn_local_with_priority(glib::source::PRIORITY_DEFAULT_IDLE, async move {
				let (progress_dialog, progress) = dinoscore::create_progress_bar_dialog("Detecting staves …");
				let selected_items = pages_preview.get_selected_items();
				let total_work = selected_items.len();
				futures::stream::iter(
					selected_items.iter()
					.map(|selected| selected.get_indices()[0] as usize)
					.enumerate()
					/* Need to manually move/clone out all GTK objects :( */
					.map(|(i, page)| (i, page, state.clone(), progress.clone(), pages_preview_data.clone()))
				)
				.for_each(|(i, page, state, progress, pages_preview_data)| async move {
					println!("Autodetecting {} ({}/{})", page, i, total_work);
	
					let bars_inner = recognize_staves(&pages_preview_data.get_value(
						&pages_preview_data.iter_nth_child(None, page as i32).unwrap(),
						0 as i32
					).downcast::<gdk_pixbuf::Pixbuf>().unwrap().get().unwrap()).await;
					state.borrow_mut().add_staves(PageIndex(page), bars_inner);
					progress.set_fraction((i+1) as f64 / total_work as f64);
				})
				.await;
				progress_dialog.emit_close();
			});
		}));

		autodetect2.connect_clicked(
			clone!(@strong state, @weak pages_preview => @default-panic, move |_button| {
				let selected_items = pages_preview.get_selected_items();
				selected_items.iter()
					.map(|selected| selected.get_indices()[0] as usize)
					.for_each(|i| {
						let bars_inner = vec![Staff {
							left: 0.0, right: 1.0, top: 0.0, bottom: 1.0,
						}];
						state.borrow_mut().add_staves(i.into(), bars_inner);
					});
			}),
		);

		let pages_preview_callback = pages_preview.connect_selection_changed(
			clone!(@strong state => @default-panic, move |pages_preview| {
				let selected_items = pages_preview.get_selected_items();
				state.borrow_mut().select_page(match selected_items.len() {
					0 => None,
					1 => Some(PageIndex(selected_items[0].get_indices()[0] as usize)),
					_ => None,
				});
			}),
		);

		pages_preview.connect_key_press_event(clone!(@strong state =>
			move |pages_preview, event| {
				if event.get_keyval() == gdk::keys::constants::Delete
				|| event.get_keyval() == gdk::keys::constants::KP_Delete {
					let state = &mut *state.borrow_mut();
					let selected_items = pages_preview.get_selected_items();
					selected_items.iter()
						.map(|selected| selected.get_indices()[0] as usize)
						.for_each(|i| {
							state.remove_page(PageIndex(i));
						});
				}
				gtk::Inhibit::default()
			}
		));

		piece_start.connect_property_active_notify(clone!(@strong state => @default-panic, move |piece_start| {
			/* If pages is already borrowed, that's because the callback was triggered from EditorState::update_widgets */
			if let Ok(mut state) = state.try_borrow_mut() {
				state.update_part_start(piece_start.get_active());
			}
		}));

		piece_name.connect_property_text_notify(clone!(@strong state  => @default-panic, move |piece_name| {
			/* If pages is already borrowed, that's because the callback was triggered from EditorState::update_widgets */
			if let Ok(mut state) = state.try_borrow_mut() {
				state.update_part_name(&piece_name.get_text());
			}
		}));

		section_start.connect_property_active_notify(clone!(@strong state => @default-panic, move |section_start| {
			/* If pages is already borrowed, that's because the callback was triggered from EditorState::update_widgets */
			if let Ok(mut state) = state.try_borrow_mut() {
				state.update_section_start(section_start.get_active());
			}
		}));
		section_repetition.connect_property_active_notify(clone!(@strong state => @default-panic, move |section_repetition| {
			/* If pages is already borrowed, that's because the callback was triggered from EditorState::update_widgets */
			if let Ok(mut state) = state.try_borrow_mut() {
				state.update_section_repetition(section_repetition.get_active());
			}
		}));
		section_end.connect_property_active_notify(clone!(@strong state => @default-panic, move |section_end| {
			/* If pages is already borrowed, that's because the callback was triggered from EditorState::update_widgets */
			if let Ok(mut state) = state.try_borrow_mut() {
				state.update_section_end(section_end.get_active());
			}
		}));

		let view = Rc::new(RefCell::new(EditorView {
			area,
			piece_name,
			piece_start,
			section_start,
			section_repetition,
			section_end,
			pages_preview,
			pages_preview_data,
			pages_preview_callback,
			state,
		}));

		editor_rx.attach(
			None,
			clone!(@strong view => move |event| {
				view.borrow_mut().update_widgets(event);
				glib::Continue(true)
			}),
		);
		view
	}

	fn update_widgets(&mut self, event: EditorViewUpdateEvent) {
		let state = &*self.state.borrow();
		println!("Update");

		match event {
			EditorViewUpdateEvent::Void => {},
			EditorViewUpdateEvent::UpdateSelection { page_changed } => {
				if page_changed {
					if let Some(selected_page) = state.selected_page {
						// let page = self.pages_preview.get_value(
						// 	&self.pages_preview.iter_nth_child(None, selected_page as i32).unwrap(),
						// 	0
						// ).downcast::<gdk_pixbuf::Pixbuf>().unwrap().get().unwrap();
						self.area.load_page(
							state.pages[*selected_page].clone(),
							state.count_staves_before(selected_page),
						);
					} else {
						self.area.unload_page();
					}
				} else {
					self.area.editor.queue_draw();
				}

				let index = state
					.selected_page
					.map(|page| state.count_staves_before(page))
					.and_then(|staff| {
						self.area
							.state
							.borrow()
							.as_ref()
							.unwrap()
							.selected_staff
							.map(|s| staff + s)
					});
				/* In Swing/JavaFX, "active"=>"selected", "sensitive"=>"enabled"/"not disabled" */

				/* Disable the check box for the first item (it's force selected there) */
				let piece_start_sensitive = index.map(|i| i > 0).unwrap_or(false);
				self.piece_start.set_sensitive(piece_start_sensitive);
				/* Set the selection */
				let piece_start_active = index
					.map(|i| state.piece_starts.get(&i.into()))
					.flatten()
					.is_some();
				self.piece_start.set_active(piece_start_active);

				/* You can only enter a name on piece starts */
				self.piece_name.set_sensitive(piece_start_active);
				self.piece_name.set_text(
					index
						.map(|i| state.piece_starts.get(&i.into()))
						.flatten()
						.map(Option::as_ref)
						.flatten()
						.map_or("", String::as_str),
				);
				/* When a piece starts, a section must start as well, so it can't be edited */
				self.section_start
					.set_sensitive(!piece_start_active && piece_start_sensitive);
				let section_start = index.and_then(|i| state.section_starts.get(&i.into()));
				self.section_start.set_active(section_start.is_some());

				self.section_repetition
					.set_sensitive(section_start.is_some());
				self.section_repetition.set_active(
					section_start
						.map(|meta| meta.is_repetition)
						.unwrap_or(false),
				);
				self.section_end.set_sensitive(section_start.is_some());
				self.section_end
					.set_active(section_start.map(|meta| meta.section_end).unwrap_or(false));
			},
			EditorViewUpdateEvent::AppendPage(pixbuf) => {
				self.pages_preview_data
					.set(&self.pages_preview_data.append(), &[0], &[&pixbuf]);
			},
			EditorViewUpdateEvent::RemovePage(index) => {
				dbg!(&index);
				self.pages_preview
					.block_signal(&self.pages_preview_callback);
				self.pages_preview_data.remove(
					&self
						.pages_preview_data
						.iter_nth_child(
							self.pages_preview_data.get_iter_first().as_ref(),
							*index as i32,
						)
						.expect("Called RemovePage event even though the list is empty"),
				);
				self.area.unload_page();
				self.pages_preview
					.unblock_signal(&self.pages_preview_callback);
			},
			EditorViewUpdateEvent::Reload => {
				self.pages_preview
					.block_signal(&self.pages_preview_callback);
				self.pages_preview_data.clear();
				for (page, _) in state.pages.iter().map(Rc::as_ref) {
					let pixbuf = pdf_to_pixbuf(&page, 400);
					self.pages_preview_data.set(
						&self.pages_preview_data.append(),
						&[0],
						&[&pixbuf],
					);
				}
				self.area.unload_page();
				self.pages_preview
					.unblock_signal(&self.pages_preview_callback);
			},
		}
	}
}

struct EditorState {
	update_ui: glib::Sender<EditorViewUpdateEvent>,
	pages: Vec<Rc<(poppler::PopplerPage, RefCell<Vec<Staff>>)>>,
	/* Sections */
	piece_starts: BTreeMap<StaffIndex, Option<String>>,
	section_starts: BTreeMap<StaffIndex, SectionMeta>,
	selected_page: Option<PageIndex>,
	area: Rc<RefCell<Option<EditorAreaState>>>,
}

impl EditorState {
	fn new(
		update_ui: glib::Sender<EditorViewUpdateEvent>,
		area: Rc<RefCell<Option<EditorAreaState>>>,
	) -> Self {
		EditorState {
			update_ui,
			pages: Vec::new(),
			piece_starts: btreemap! {0.into() => None},
			section_starts: btreemap! {0.into() => SectionMeta::default()},
			selected_page: None,
			area,
		}
	}

	fn unload_new(&mut self) {
		self.pages = vec![];
		self.piece_starts = btreemap! {0.into() => None};
		self.section_starts = btreemap! {0.into() => SectionMeta::default()};
		*self.area.borrow_mut() = None;
		self.selected_page = None;
		self.update_ui.send(EditorViewUpdateEvent::Reload).unwrap();
	}

	fn load_with_dialog(&mut self) {
		let filter = gtk::FileFilter::new();
		filter.add_mime_type("application/zip");
		let choose = gtk::FileChooserNativeBuilder::new()
			.title("File to load")
			.action(gtk::FileChooserAction::Open)
			.select_multiple(false)
			.filter(&filter)
			.build();
		if choose.run() == gtk::ResponseType::Accept {
			if let Some(file) = choose.get_file() {
				let path = file.get_path().unwrap();
				let (pdf, meta) = block_on(load_song(path));
				self.load(pdf, meta);
			}
		}
	}

	fn load(&mut self, pages: poppler::PopplerDocument, song: SongMeta) {
		self.pages = (0..pages.get_n_pages())
			.map(|index| {
				(
					pages.get_page(index).unwrap(),
					RefCell::new(
						song.staves
							.iter()
							.filter(|line| line.page == index.into())
							.map(|line| Staff {
								left: line.start.0,
								right: line.end.0,
								top: line.start.1,
								bottom: line.end.1,
							})
							.collect::<Vec<_>>(),
					),
				)
			})
			.map(Rc::new)
			.collect();
		self.piece_starts = song.0.piece_starts;
		self.section_starts = song.0.section_starts;
		self.selected_page = None;
		*self.area.borrow_mut() = None;
		self.update_ui.send(EditorViewUpdateEvent::Reload).unwrap();
	}

	fn save_with_ui(&mut self) {
		println!("Saving staves");

		let filter = gtk::FileFilter::new();
		filter.add_mime_type("application/zip");
		let choose = gtk::FileChooserNativeBuilder::new()
			.title("Save song")
			.action(gtk::FileChooserAction::Save)
			.select_multiple(false)
			.do_overwrite_confirmation(true)
			.filter(&filter)
			.build();
		if choose.run() == gtk::ResponseType::Accept {
			if let Some(file) = choose.get_file() {
				save_song(
					file.get_path().unwrap(),
					SongMeta(SongMetaV1 {
						staves: self.get_lines(),
						piece_starts: self.piece_starts.clone(),
						section_starts: self.section_starts.clone(),
					}),
					self.pages.iter().map(|page| &page.0),
				);
			}
		}
	}

	fn add_page(&mut self, page: poppler::PopplerPage) {
		let pixbuf = pdf_to_pixbuf(&page, 400);
		self.pages.push(Rc::new((page, RefCell::new(vec![]))));
		self.update_ui
			.send(EditorViewUpdateEvent::AppendPage(pixbuf))
			.unwrap();
	}

	fn remove_page(&mut self, index: PageIndex) {
		self.pages.remove(*index);
		self.update_ui
			.send(EditorViewUpdateEvent::RemovePage(index))
			.unwrap();
	}

	fn select_page(&mut self, selected_page: Option<PageIndex>) {
		self.selected_page = selected_page;
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection { page_changed: true })
			.unwrap();
	}

	fn add_staves(&mut self, page_index: PageIndex, mut staves: Vec<Staff>) {
		self.pages[*page_index].1.borrow_mut().append(&mut staves);
		if self.selected_page == Some(page_index) {
			self.update_ui
				.send(EditorViewUpdateEvent::UpdateSelection {
					page_changed: false,
				})
				.unwrap();
		}
	}

	fn get_lines(&self) -> Vec<Line> {
		self.pages
			.iter()
			.enumerate()
			.flat_map(|(page_index, page)| {
				page.1
					.borrow()
					.iter()
					.map(move |staff| Line {
						page: page_index.into(),
						start: (staff.left, staff.top),
						end: (staff.right, staff.bottom),
					})
					// TODO improve
					.collect::<Vec<_>>()
					.into_iter()
			})
			.collect()
	}

	fn count_staves_before(&self, page: PageIndex) -> usize {
		self.pages[0..*page]
			.iter()
			.map(|p| p.1.borrow().len())
			.sum()
	}

	fn update_part_name(&mut self, new_name: &str) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.area
					.borrow()
					.as_ref()
					.unwrap()
					.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let name = self
			.piece_starts
			.get_mut(&index.into())
			.expect("You shouldn't be able to set the name on non part starts");
		*name = Some(new_name.to_string());
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection {
				page_changed: false,
			})
			.unwrap();
	}

	fn update_section_start(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.area
					.borrow()
					.as_ref()
					.unwrap()
					.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		if selected {
			self.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.section_starts.remove(&index);
		}
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection {
				page_changed: false,
			})
			.unwrap();
	}

	fn update_section_repetition(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.area
					.borrow()
					.as_ref()
					.unwrap()
					.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		self.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.is_repetition = selected;
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection {
				page_changed: false,
			})
			.unwrap();
	}

	fn update_section_end(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.area
					.borrow()
					.as_ref()
					.unwrap()
					.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		self.section_starts
			.get_mut(&index)
			.expect("You shouldn't be able to click this if there's no section start")
			.section_end = selected;
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection {
				page_changed: false,
			})
			.unwrap();
	}

	fn update_part_start(&mut self, selected: bool) {
		let index = self
			.selected_page
			.map(|page| self.count_staves_before(page))
			.and_then(|staff| {
				self.area
					.borrow()
					.as_ref()
					.unwrap()
					.selected_staff
					.map(|s| staff + s)
			})
			.expect("You shouldn't be able to click this with nothing selected");
		let index = StaffIndex(index);
		if selected {
			self.piece_starts.entry(index).or_insert(None);
			/* When a piece starts, a section must start as well */
			self.section_starts
				.entry(index)
				.or_insert_with(SectionMeta::default);
		} else {
			self.piece_starts.remove(&index);
		}
		self.update_ui
			.send(EditorViewUpdateEvent::UpdateSelection {
				page_changed: false,
			})
			.unwrap();
	}
}

async fn load_song<P: AsRef<std::path::Path>>(path: P) -> (poppler::PopplerDocument, SongMeta) {
	let mut song = zip::read::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
	// I'm tired, okay?
	// TODO wtf
	let (pages, mut song) = {
		let (data, song) = unblock! {
			let data = {
				let mut pages = song.by_name("sheet.pdf").unwrap();
				let mut data: Vec<u8> = vec![];
				std::io::copy(&mut pages, &mut data).unwrap();
				let data: &mut [u8] = &mut *Box::leak(data.into_boxed_slice()); // TODO: absolutely remove this
				data
			};
			(data, song)
		};
		(
			poppler::PopplerDocument::new_from_data(data, "").unwrap(),
			song,
		)
	};
	let metadata: SongMeta =
		unblock! { serde_json::from_reader(song.by_name("staves.json").unwrap()).unwrap() };
	(pages, metadata)
}

fn save_song<'a, P: AsRef<std::path::Path>>(
	path: P,
	metadata: SongMeta,
	pages: impl Iterator<Item = &'a poppler::PopplerPage>,
) {
	let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
	writer
		.start_file("staves.json", zip::write::FileOptions::default())
		.unwrap();
	serde_json::to_writer(&mut writer, &metadata).unwrap();

	println!("Saving sheets");
	writer
		.start_file("sheet.pdf", zip::write::FileOptions::default())
		.unwrap();
	let surface = cairo::PdfSurface::for_stream(500.0, 500.0, writer).unwrap();
	let context = cairo::Context::new(&surface);
	for page in pages {
		surface
			.set_size(page.get_size().0, page.get_size().1)
			.unwrap();
		page.render(&context);
		context.show_page();
	}
	surface.flush();
	writer = *surface
		.finish_output_stream()
		.unwrap()
		.downcast::<zip::ZipWriter<std::fs::File>>()
		.unwrap();

	writer.finish().unwrap();
}

fn build_ui(application: &gtk::Application) {
	let builder = gtk::Builder::from_file("res/editor.glade");
	let window: gtk::Window = builder.get_object("window").unwrap();
	window.set_application(Some(application));
	window.set_position(gtk::WindowPosition::Center);
	application.set_menubar(Some(
		&builder.get_object::<gio::MenuModel>("menubar").unwrap(),
	));

	let editor_view = EditorView::new(&builder);

	let new = gio::SimpleAction::new("new", None);
	new.connect_activate(
		clone!(@weak editor_view, @weak application => @default-panic, move |_action, _parameter| {
			editor_view.borrow()
				.state.borrow_mut()
				.unload_new();
		}),
	);
	application.add_action(&new);
	application.set_accels_for_action("app.new", &["<Primary>N"]);

	let open = gio::SimpleAction::new("open", None);
	open.connect_activate(
		clone!(@weak editor_view, @weak application => @default-panic, move |_action, _parameter| {
			editor_view.borrow()
				.state.borrow_mut()
				.load_with_dialog();
		}),
	);
	application.add_action(&open);
	application.set_accels_for_action("app.open", &["<Primary>O"]);

	let save = gio::SimpleAction::new("save", None);
	save.connect_activate(
		clone!(@weak editor_view, @weak application => @default-panic, move |_action, _parameter| {
			editor_view.borrow()
				.state.borrow_mut()
				.save_with_ui();
		}),
	);
	application.add_action(&save);
	application.set_accels_for_action("app.save", &["<Primary>S"]);

	// let pieces_list: gtk::ListBox = builder.get_object("pieces_list").unwrap();
	// let pieces = gio::ListStore::new(row_data::RowData::static_type());
	// pieces_list.bind_model(Some(&pieces), |item| {
	// 	use row_data::RowData;

	// 	let row = gtk::ListBoxRow::new();
	// 	let item = item.downcast_ref::<RowData>().expect("Row data is of wrong type");

	// 	let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 5);

	// 	let entry = gtk::Entry::new();
	// 	item.bind_property("name", &entry, "text")
	// 		.flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE | glib::BindingFlags::BIDIRECTIONAL)
	// 		.build();
	// 	hbox.pack_start(&entry, false, false, 0);

	// 	let spin_button_start = gtk::SpinButton::new(Some(&gtk::Adjustment::new(0.0, 0.0, u32::max_value() as f64, 1.0, 1.0, 1.0)), 1.0, 0);
	// 	item.bind_property("start", &spin_button_start, "value")
	// 		.flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE | glib::BindingFlags::BIDIRECTIONAL)
	// 		.build();
	// 	hbox.pack_end(&spin_button_start, false, false, 0);

	// 	let spin_button_end = gtk::SpinButton::new(Some(&gtk::Adjustment::new(0.0, 0.0, u32::max_value() as f64, 1.0, 1.0, 1.0)), 1.0, 0);
	// 	item.bind_property("end", &spin_button_end, "value")
	// 		.flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE | glib::BindingFlags::BIDIRECTIONAL)
	// 		.build();
	// 	hbox.pack_end(&spin_button_end, false, false, 0);

	// 	let from_selection = gtk::Button::with_label("From selection");
	// 	from_selection.set_tooltip_text(Some("Set the range from the selected staves"));
	// 	hbox.pack_end(&from_selection, false, false, 0);

	// 	row.add(&hbox);
	// 	row.show_all();
	// 	row.upcast::<gtk::Widget>()
	// });

	// let pieces_add: gtk::Button = builder.get_object("pieces_add").unwrap();
	// pieces_add.connect_clicked(clone!(@strong pieces => move |_pieces_add| {
	// 	let count = pieces.get_n_items() + 1;
	// 	let th = match count % 100 {
	// 		11 | 12 | 13 => "th",
	// 		_ => match count % 10 {
	// 			1 => "st", 2 => "nd", 3 => "rd", _ => "th"
	// 		}
	// 	};
	// 	pieces.append(&row_data::RowData::new(&format!("{}{} movement", count, th), 0, 0));
	// }));

	window.show_all();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Staff {
	left: f64,
	top: f64,
	right: f64,
	bottom: f64,
}

const PB_PATH: &str = "./res/2019-05-16_faster-rcnn-inception-resnet-v2.pb";

static DETECTION_GRAPH: once_cell::sync::Lazy<tensorflow::Graph> =
	once_cell::sync::Lazy::new(|| {
		use std::io::Read;
		use tensorflow as tf;

		let mut detection_graph = tf::Graph::new();
		let mut proto = Vec::new();
		std::fs::File::open(PB_PATH)
			.unwrap()
			.read_to_end(&mut proto)
			.unwrap();
		dbg!("c");
		detection_graph
			.import_graph_def(&proto, &tf::ImportGraphDefOptions::new())
			.unwrap();
		detection_graph
	});

async fn recognize_staves(image: &gdk_pixbuf::Pixbuf) -> Vec<Staff> {
	assert!(!image.get_has_alpha());
	assert!(image.get_n_channels() == 3);
	assert!(image.get_colorspace() == gdk_pixbuf::Colorspace::Rgb);

	use tensorflow as tf;

	let image_bytes = &image.read_pixel_bytes().unwrap();

	let detection_graph = &DETECTION_GRAPH;

	let image_tensor =
		tf::Tensor::new(&[1, image.get_height() as u64, image.get_width() as u64, 3])
			.with_values(image_bytes)
			.unwrap();

	let mut session = tf::Session::new(&tf::SessionOptions::new(), &detection_graph).unwrap();
	let mut session_args = tf::SessionRunArgs::new();
	session_args.add_feed::<u8>(
		&detection_graph
			.operation_by_name("image_tensor")
			.unwrap()
			.unwrap(),
		0,
		&image_tensor,
	);

	let num_detections = session_args.request_fetch(
		&detection_graph
			.operation_by_name("num_detections")
			.unwrap()
			.unwrap(),
		0,
	);
	let detection_boxes = session_args.request_fetch(
		&detection_graph
			.operation_by_name("detection_boxes")
			.unwrap()
			.unwrap(),
		0,
	);
	let detection_scores = session_args.request_fetch(
		&detection_graph
			.operation_by_name("detection_scores")
			.unwrap()
			.unwrap(),
		0,
	);
	let detection_classes = session_args.request_fetch(
		&detection_graph
			.operation_by_name("detection_classes")
			.unwrap()
			.unwrap(),
		0,
	);

	session.run(&mut session_args).unwrap();

	/* We could probably extract better results by making more use of all that information */
	let num_detections = session_args.fetch::<f32>(num_detections).unwrap();
	let detection_boxes = session_args.fetch::<f32>(detection_boxes).unwrap();
	let detection_scores = session_args.fetch::<f32>(detection_scores).unwrap();
	let _detection_classes = session_args.fetch::<f32>(detection_classes).unwrap();

	session.close().unwrap();

	let mut bars = Vec::<Staff>::new();

	for i in 0..(num_detections[0] as usize) {
		if detection_scores[i] > 0.6 {
			let detected = &detection_boxes[i * 4..i * 4 + 4];
			let y1 = detected[0] * image.get_height() as f32;
			let x1 = detected[1] * image.get_width() as f32;
			let y2 = detected[2] * image.get_height() as f32;
			let x2 = detected[3] * image.get_width() as f32;

			bars.push(Staff {
				left: x1 as f64,
				top: y1 as f64,
				bottom: y2 as f64,
				right: x2 as f64,
			});
		}
	}

	let scale_x = 1.0 / image.get_width() as f64;
	let scale_y = 1.0 / image.get_height() as f64;

	unblock! {
		for bar in &mut bars {
			bar.left *= scale_x;
			bar.top *= scale_y;
			bar.right *= scale_x;
			bar.bottom *= scale_y;
		}

		/* Group them by staff */
		let mut bars = bars.into_iter().enumerate().collect::<Vec<_>>();

		while { /* do */
			let mut changed = false;
			for i in 0..bars.len() {
				for j in 0..bars.len() {
					if i == j {
						continue;
					}
					// This is safe thanks to the index check above
					let bar1 = & unsafe { &*(&bars as *const Vec<(usize, Staff)>) }[i];
					let bar2 = &mut unsafe { &mut *(&mut bars as *mut Vec<(usize, Staff)>) }[j];
					let c1 = (bar1.1.top + bar1.1.bottom) / 2.0;
					let c2 = (bar2.1.top + bar2.1.bottom) / 2.0;
					if c1 > bar2.1.top && c1 < bar2.1.bottom
							&& c2 > bar1.1.top && c2 < bar1.1.bottom
							&& bar1.0 != bar2.0 {
						changed = true;
						bar2.0 = bar1.0;
					}
				}
			}
			/* while */
			changed
		} {};

		let staves = bars.into_iter().into_group_map();

		/* Merge them */
		use reduce::Reduce;
		let mut staves: Vec<Staff> = staves.into_iter().filter_map(
			|staves| {
				staves.1.into_iter().reduce(|a, b| Staff {
					left: a.left.min(b.left),
					right: a.right.max(b.right),
					top: a.top.min(b.top),
					bottom: a.bottom.max(b.bottom),
				})
			})
			.collect();
		staves.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap());

		/* Overlapping is bad */
		(0..staves.len()).collect::<Vec<_>>()
			.windows(2)
			.for_each(|idx| {
				macro_rules! staff_a (() => {staves[idx[0]]});
				macro_rules! staff_b (() => {staves[idx[1]]});

				if staff_a!().bottom > staff_b!().top
					&& /* 90% horizontal overlap */
					(f64::min(staff_a!().right, staff_b!().right) - f64::max(staff_a!().left, staff_b!().left)) / (f64::max(staff_a!().right, staff_b!().right) - f64::min(staff_a!().left, staff_b!().left)) > 0.9
				{
					let center = (staff_a!().bottom + staff_b!().top) / 2.0;
					staff_a!().bottom = center;
					staff_b!().top = center;
				}
			});

		/* Fixup fuckups */
		for staff in &mut staves {
			if staff.top > staff.bottom {
				std::mem::swap(&mut staff.top, &mut staff.bottom);
			}
			if staff.left > staff.right {
				std::mem::swap(&mut staff.left, &mut staff.right);
			}
		}

		staves
	}
}

/// Create a PDF Document with a single page that wraps a raster image
fn pixbuf_to_pdf(image: &gdk_pixbuf::Pixbuf) -> poppler::PopplerDocument {
	let pdf_binary: Vec<u8> = Vec::new();
	let surface = cairo::PdfSurface::for_stream(
		image.get_width() as f64,
		image.get_height() as f64,
		pdf_binary,
	)
	.unwrap();

	let context = cairo::Context::new(&surface);
	context.set_source_pixbuf(image, 0.0, 0.0);
	context.paint();

	surface.flush();

	let pdf_binary = surface
		.finish_output_stream()
		.unwrap()
		.downcast::<Vec<u8>>()
		.unwrap();
	let pdf_binary: &mut [u8] = &mut *Box::leak(pdf_binary); // TODO: absolutely remove this

	poppler::PopplerDocument::new_from_data(pdf_binary, "").unwrap()
}

/// Render a PDF page to a preview image with fixed width
fn pdf_to_pixbuf(page: &poppler::PopplerPage, width: i32) -> gdk_pixbuf::Pixbuf {
	let surface = cairo::ImageSurface::create(
		cairo::Format::Rgb24,
		width,
		(width as f64 * page.get_size().1 / page.get_size().0) as i32,
	)
	.unwrap();
	let context = cairo::Context::new(&surface);
	let scale = width as f64 / page.get_size().0;
	context.set_antialias(cairo::Antialias::Best);
	context.scale(scale, scale);
	context.set_source_rgb(1.0, 1.0, 1.0);
	context.paint();
	page.render(&context);
	surface.flush();

	gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.get_width(), surface.get_height()).unwrap()
}

fn main() {
	let application = gtk::Application::new(
		Some("de.piegames.dinoscore.viewer"),
		gio::ApplicationFlags::NON_UNIQUE,
	)
	.expect("Initialization failed...");

	// let editor = true;
	application.connect_activate(move |app| {
		build_ui(app);
	});

	// When activated, shuts down the application
	let quit = gio::SimpleAction::new("quit", None);
	quit.connect_activate(
		clone!(@weak application => @default-panic, move |_action, _parameter| {
			application.quit();
		}),
	);
	application.add_action(&quit);
	application.connect_startup(|application| {
		libhandy::init();
		application.set_accels_for_action("app.quit", &["<Primary>Q"]);
	});

	// application.run(&args().collect::<Vec<_>>());
	application.run(&[]);
}

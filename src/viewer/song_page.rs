use dinoscore::{prelude::*, *};

#[derive(Debug)]
pub struct PageLayout {
	/// The current page to be displayed
	pub page: layout::PageIndex,
	/// Where to render the staves
	pub staves: Vec<layout::StaffLayout>,
	/// The width this layout was made for
	pub width: i32,
	/// The height this layout was made for
	pub height: i32,
}

glib::wrapper! {
	pub struct SongPage(ObjectSubclass<imp::SongPage>)
		@extends gtk::Widget,
		@implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
					gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SongPage {
	pub fn new(
		song: Arc<collection::SongMeta>,
		layout: PageLayout,
		pages: Rc<
			TiVec<
				collection::PageIndex,
				RefCell<(Option<(gdk::Texture, f64)>, Option<poppler::Page>)>,
			>,
		>,
	) -> Self {
		let obj: Self = Object::new(&[]).unwrap();
		obj.imp().song.set(song).unwrap();
		obj.imp().pages.set(pages).unwrap();
		obj.update_layout(layout);
		obj
	}

	pub fn update_layout(&self, layout: PageLayout) {
		*self.imp().layout.borrow_mut() = Some(layout);
		self.queue_draw();
	}
}

mod imp {
	use super::*;

	#[derive(Default)]
	pub struct SongPage {
		pub song: OnceCell<Arc<collection::SongMeta>>,
		pub layout: RefCell<Option<PageLayout>>,
		pub pages: OnceCell<
			Rc<
				TiVec<
					collection::PageIndex,
					RefCell<(Option<(gdk::Texture, f64)>, Option<poppler::Page>)>,
				>,
			>,
		>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for SongPage {
		const NAME: &'static str = "ViewerSongPage";
		type Type = super::SongPage;
		type ParentType = gtk::Widget;

		fn class_init(_klass: &mut Self::Class) {}

		fn instance_init(_obj: &InitializingObject<Self>) {}
	}

	impl ObjectImpl for SongPage {
		fn properties() -> &'static [glib::ParamSpec] {
			Box::leak(Box::new([]))
		}

		fn constructed(&self, obj: &Self::Type) {
			self.parent_constructed(obj);
			obj.set_hexpand(true);
			obj.set_vexpand(true);
		}
	}

	impl WidgetImpl for SongPage {
		fn snapshot(&self, obj: &Self::Type, snapshot: &gtk::Snapshot) {
			self.parent_snapshot(obj, snapshot);

			/* Zero sizes cause problems */
			if obj.width() < 1 || obj.height() < 1 {
				return;
			}
			let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
			/* Make sure we don't render outside of out widget */
			snapshot.push_clip(&bounds);

			let layout = self.layout.borrow();
			let layout = layout.as_ref().unwrap();

			/* The actual rendering code. Might be called twice for dark mode */
			let render = || {
				snapshot.append_color(&gdk::RGBA::WHITE, &bounds);
				layout
					.staves
					.iter()
					.try_for_each(|staff_layout| {
						snapshot.save();
						/* Point origin at staff start */
						snapshot.translate(&graphene::Point::new(
							staff_layout.x as f32,
							staff_layout.y as f32,
						));

						/* Staff */
						snapshot.save();
						let staff = &self.song.get().unwrap().staves[staff_layout.index];
						let (rendered_page, annotations) =
							&*self.pages.get().unwrap()[staff.page].borrow();
						match rendered_page.as_ref() {
							Some((page, page_scale)) => {
								/* Render the image */
								snapshot.push_clip(&graphene::Rect::new(
									0.0,
									0.0,
									staff_layout.width as f32,
									staff_layout.width as f32 * staff.aspect_ratio() as f32,
								));
								let scale = staff_layout.width as f32 / staff.width() as f32;
								snapshot.scale(scale, scale);
								snapshot.append_texture(
									page,
									&graphene::Rect::new(
										-staff.start.0 as f32,
										-staff.start.1 as f32,
										page.width() as f32 * *page_scale as f32,
										page.height() as f32 * *page_scale as f32,
									),
								);
								snapshot.pop();
							},
							None => {
								/* Render a placeholder */
								snapshot.append_color(
									&gdk::RGBA::new(0.8, 0.8, 0.8, 1.0),
									&graphene::Rect::new(
										0.0,
										0.0,
										staff_layout.width as f32,
										staff_layout.width as f32 * staff.aspect_ratio() as f32,
									),
								);
							},
						}
						snapshot.restore();

						/* Page/Staff number */
						let context = snapshot.append_cairo(&bounds);
						context.set_font_size(20.0);
						context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
						context.move_to(10.0, 16.0);
						let (page_index, staff_index) =
							self.song.get().unwrap().page_of_piece(staff_layout.index);
						context.show_text(&format!("{}-{}", *page_index + 1, *staff_index))?;

						snapshot.restore();

						/* Render annotations */
						if let Some(page) = annotations.as_ref() {
							let context = snapshot.append_cairo(&bounds);

							context.translate(staff_layout.x, staff_layout.y);

							let scale = staff_layout.width / staff.width();
							context.scale(scale, scale);
							context.translate(-staff.start.0, -staff.start.1);

							context.rectangle(
								staff.start.0,
								staff.start.1,
								staff.width(),
								staff.height(),
							);
							context.clip();

							page.render(&context);
						}

						cairo::Result::Ok(())
					})
					.expect("Failed to draw");
			};

			if adw::StyleManager::default().is_dark() {
				/* Dark mode: Invert luminosity by inverting colors + blending */
				snapshot.push_blend(gsk::BlendMode::Luminosity);
				render();
				snapshot.pop();
				snapshot.push_color_matrix(
					&graphene::Matrix::new_scale(-1.0, -1.0, -1.0),
					&graphene::Vec4::one(),
				);
				render();
				snapshot.pop();
				snapshot.pop();
			} else {
				render();
			}

			snapshot.pop();
		}
	}

	impl SongPage {}
}

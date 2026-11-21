use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{graphene, gsk};

glib::wrapper! {
    pub struct ZoomView(ObjectSubclass<imp::ZoomView>)
        @extends gtk::Widget,
        @implements gtk::Buildable, gtk::ConstraintTarget;
}

impl ZoomView {
    pub fn new(child: &impl IsA<gtk::Widget>, base_width: i32) -> Self {
        let view: Self = glib::Object::builder().build();
        child.set_parent(&view);
        let imp = view.imp();
        imp.base_width.set(base_width.max(1));
        imp.child.replace(Some(child.clone().upcast()));
        view
    }

    pub fn set_scale(&self, scale: f64) {
        let scale = scale.clamp(0.25, 4.0);
        let imp = self.imp();
        if (imp.scale.get() - scale).abs() < f64::EPSILON {
            return;
        }
        imp.scale.set(scale);
        self.queue_resize();
        self.queue_draw();
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ZoomView {
        pub child: RefCell<Option<gtk::Widget>>,
        pub base_width: Cell<i32>,
        pub scale: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZoomView {
        const NAME: &'static str = "DocxLiteZoomView";
        type Type = super::ZoomView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ZoomView {
        fn constructed(&self) {
            self.parent_constructed();
            self.scale.set(1.0);
            let view = self.obj();
            view.set_hexpand(true);
            view.set_vexpand(true);
        }

        fn dispose(&self) {
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ZoomView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let scale = self.scale.get();
            let base_width = self.base_width.get().max(1);
            match orientation {
                gtk::Orientation::Horizontal => {
                    let width = scale_size(base_width, scale);
                    (MINIMUM_SIZE.min(width), width, -1, -1)
                }
                _ => {
                    let height = self
                        .child
                        .borrow()
                        .as_ref()
                        .map(|child| {
                            let (_, natural, _, _) =
                                child.measure(gtk::Orientation::Vertical, base_width);
                            scale_size(natural, scale)
                        })
                        .unwrap_or(0);
                    (MINIMUM_SIZE.min(height), height, -1, -1)
                }
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let child = self.child.borrow().clone();
            let Some(child) = child else {
                return;
            };
            let scale = self.scale.get();
            if scale <= 0.0 {
                return;
            }
            let base_width = self.base_width.get().max(1);
            let (_, natural_height, _, _) = child.measure(gtk::Orientation::Vertical, base_width);
            let base_height = natural_height.max(1);
            let scaled_width = scale_size(base_width, scale);
            let offset_x = (f64::from(width - scaled_width) / 2.0).round().max(0.0) as f32;
            let transform = gsk::Transform::new()
                .translate(&graphene::Point::new(offset_x, 0.0))
                .scale(scale as f32, scale as f32);
            child.allocate(base_width, base_height, -1, Some(transform));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let child = self.child.borrow().clone();
            if let Some(child) = child {
                self.obj().snapshot_child(&child, snapshot);
            }
        }
    }
}

const MINIMUM_SIZE: i32 = 200;

fn scale_size(size: i32, scale: f64) -> i32 {
    (f64::from(size) * scale).round().max(0.0) as i32
}

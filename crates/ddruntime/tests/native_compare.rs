//! Pure transport/state-machine fixtures; no game entry or GPU.
use d3d9cmd::*;
use ddruntime::ndraw_native::comparison::Comparison;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Begin(u64, u32, u32),
    Oracle(u32),
    End(u64, usize),
    Upload,
}
struct Backend {
    events: Arc<Mutex<Vec<Event>>>,
    begin_ok: bool,
    end_ok: bool,
}
impl GraphicsBackend for Backend {
    fn native_compare_begin(&mut self, b: NativeCompareBegin) -> bool {
        let target = b.colours[0].as_ref().unwrap();
        self.events
            .lock()
            .unwrap()
            .push(Event::Begin(b.id, target.surface, target.level));
        self.begin_ok
    }
    fn native_compare_end(&mut self, id: u64, job: NativeJob) -> Result<(), NativeJob> {
        assert_eq!(job.passes.len(), 1);
        assert!(job.passes[0].clear_colour.is_none());
        self.events
            .lock()
            .unwrap()
            .push(Event::End(id, job.passes[0].draws.len()));
        if self.end_ok {
            Ok(())
        } else {
            Err(job)
        }
    }
    fn upload(&mut self, _: Upload) {
        self.events.lock().unwrap().push(Event::Upload);
    }
    fn draw(&mut self, d: &DrawCall) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Oracle(d.targets[0].as_ref().unwrap().surface));
    }
    fn clear(&mut self, _: &ClearCall) {}
    fn present(&mut self, _: &Target) {}
    fn copy(&mut self, _: &CopyCall) {}
    fn fill(&mut self, _: &FillCall) {}
    fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> {
        None
    }
    fn query_begin(&mut self, _: u32) {}
    fn query_end(&mut self, _: u32) {}
    fn query_result(&mut self, _: u32) -> Option<u64> {
        None
    }
    fn gamma(&mut self, _: Option<&GammaRamp>) {}
    fn release(&mut self, _: u32) {}
}
fn fixture() -> (NativeDraw, DrawCall) {
    let n = NativeDraw {
        state: Default::default(),
        vertex_shader: Arc::from([]),
        pixel_shader: Arc::from([]),
        declaration: Arc::from([]),
        textures: Arc::from([]),
        streams: Arc::from([]),
        indices: None,
        vs_constants: Default::default(),
        ps_constants: Default::default(),
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: None,
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        instances: 1,
    };
    let d = DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Streams(Arc::from([])),
        indices: Indices::None,
        vertex_shader: Some(n.vertex_shader.clone()),
        pixel_shader: Some(n.pixel_shader.clone()),
        declaration: n.declaration.clone(),
        fvf: 0,
        vs_constants: Default::default(),
        ps_constants: Default::default(),
        textures: Arc::from([]),
        render_states: Vec::new().into(),
        viewport: n.viewport,
        scissor: [0, 0, 8, 8],
        targets: [
            Some(Target {
                surface: 7,
                format: 21,
                width: 8,
                height: 8,
                texture: None,
                level: 0,
                samples: 0,
                quality: 0,
            }),
            None,
            None,
            None,
        ],
        depth: None,
    };
    (n, d)
}
fn backend() -> Backend {
    Backend {
        events: Default::default(),
        begin_ok: true,
        end_ok: true,
    }
}

#[test]
fn markers_bracket_oracle_writes_and_split_changed_subresources() {
    let mut b = backend();
    let mut c = Comparison::default();
    let (native, mut oracle) = fixture();
    c.draw(&mut b, native.clone(), &oracle);
    c.draw(&mut b, native.clone(), &oracle);
    oracle.targets[0].as_mut().unwrap().level = 1;
    c.draw(&mut b, native, &oracle);
    c.finish(&mut b);
    c.finish(&mut b);
    assert_eq!(
        *b.events.lock().unwrap(),
        vec![
            Event::Begin(0, 7, 0),
            Event::Oracle(7),
            Event::Oracle(7),
            Event::End(0, 2),
            Event::Begin(1, 7, 1),
            Event::Oracle(7),
            Event::End(1, 1)
        ]
    );
    assert!(!c.is_active());
    assert_eq!(
        (
            c.stats.begun,
            c.stats.submitted,
            c.stats.rejected,
            c.stats.oracle_draws,
            c.stats.shadow_draws
        ),
        (2, 2, 0, 3, 3)
    );
}

#[test]
fn rejected_comparisons_still_execute_oracle_exactly_once() {
    let mut b = backend();
    let mut c = Comparison::default();
    let (native, oracle) = fixture();
    b.begin_ok = false;
    c.draw(&mut b, native.clone(), &oracle);
    c.finish(&mut b);
    b.begin_ok = true;
    b.end_ok = false;
    c.draw(&mut b, native, &oracle);
    c.finish(&mut b);
    assert_eq!(
        *b.events.lock().unwrap(),
        vec![
            Event::Begin(0, 7, 0),
            Event::Oracle(7),
            Event::Begin(1, 7, 0),
            Event::Oracle(7),
            Event::End(1, 1)
        ]
    );
    assert_eq!(
        (c.stats.rejected, c.stats.oracle_draws, c.stats.shadow_draws),
        (2, 2, 0)
    );
}

#[test]
fn deferred_boundaries_precede_dirty_uploads_and_fallback_commands() {
    let b = backend();
    let events = b.events.clone();
    let mut b = DeferredBackend::new(Box::new(b));
    let mut c = Comparison::default();
    let (native, oracle) = fixture();
    c.draw(&mut b, native.clone(), &oracle);
    // Sink closes the pass before an upload can change any pending input.
    c.finish(&mut b);
    b.upload(Upload::Buffer {
        resource: 1,
        bytes: vec![7],
    });
    c.draw(&mut b, native, &oracle);
    c.finish(&mut b);
    b.draw(&oracle); // fallback draw occurs after comparison end
    b.fence();
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            Event::Begin(0, 7, 0),
            Event::Oracle(7),
            Event::End(0, 1),
            Event::Upload,
            Event::Begin(1, 7, 0),
            Event::Oracle(7),
            Event::End(1, 1),
            Event::Oracle(7)
        ]
    );
}

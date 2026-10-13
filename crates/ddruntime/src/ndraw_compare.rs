//! FIFO producer for L127. The D3D9 oracle always renders exactly once,
//! including when comparison admission or native execution is refused.
use d3d9cmd::{DrawCall, GraphicsBackend, NativeCompareBegin, NativeDraw, NativeJob, NativePass};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub begun: u64,
    pub submitted: u64,
    pub rejected: u64,
    pub oracle_draws: u64,
    pub shadow_draws: u64,
}

#[derive(Default)]
pub struct Comparison {
    next_id: u64,
    active: Option<(u64, NativePass)>,
    pub stats: Stats,
}

impl Comparison {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
    /// Start before the first oracle write, coalesce only identical attachments,
    /// and end before a target change. The caller also ends before uploads that
    /// could replace resources an earlier shadow draw still needs.
    pub fn draw(&mut self, backend: &mut dyn GraphicsBackend, native: NativeDraw, oracle: &DrawCall) {
        if self
            .active
            .as_ref()
            .is_some_and(|(_, p)| p.colours != oracle.targets || p.depth != oracle.depth)
        {
            self.finish(backend);
        }
        if self.active.is_none() {
            let id = self.next_id;
            self.next_id = self
                .next_id
                .checked_add(1)
                .expect("native comparison pass counter exhausted");
            let begin = NativeCompareBegin {
                id,
                colours: oracle.targets.clone(),
                depth: oracle.depth.clone(),
            };
            if backend.native_compare_begin(begin) {
                self.stats.begun += 1;
                self.active = Some((
                    id,
                    NativePass {
                        colours: oracle.targets.clone(),
                        depth: oracle.depth.clone(),
                        ..Default::default()
                    },
                ));
            } else {
                self.stats.rejected += 1;
                if self.stats.rejected <= 16 {
                    eprintln!("native-compare unverified pass={id}: begin rejected; D3D9 oracle retained");
                }
            }
        }
        backend.draw(oracle);
        self.stats.oracle_draws += 1;
        if let Some((_, pass)) = &mut self.active {
            pass.draws.push(native);
        }
    }

    /// Before a fallback command, dirty upload, target change, or frame end.
    /// There is no D3D9 fallback on rejection: the oracle already executed.
    pub fn finish(&mut self, backend: &mut dyn GraphicsBackend) {
        let Some((id, pass)) = self.active.take() else {
            return;
        };
        let draws = pass.draws.len() as u64;
        if backend
            .native_compare_end(id, NativeJob { passes: vec![pass] })
            .is_ok()
        {
            self.stats.submitted += 1;
            self.stats.shadow_draws += draws;
        } else {
            self.stats.rejected += 1;
            if self.stats.rejected <= 16 {
                eprintln!("native-compare unverified pass={id} draws={draws}: native rejected; D3D9 oracle retained");
            }
        }
    }
}

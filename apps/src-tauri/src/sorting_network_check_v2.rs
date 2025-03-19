#![allow(unused)]

use crate::{sorting_network_check_v2, threadpool};
use rayon::prelude::*;
use serde::{Deserialize, Serialize, de::Error};
use std::sync::{Arc, Mutex, mpsc};

pub type State = u64;

#[derive(Debug, Clone)]
pub struct JobResult {
    pub time: u64,
    // Number of branches examined: 0 <= branches <= FIB1[n]
    pub progress: u64,
    pub progress_all: u64,
    // Whether comparator i was used
    pub used: Vec<bool>,
    // Bitmap of positions where z, o are not sorted
    pub unsorted: [State; State::BITS as usize],
    pub log: String,
}

#[derive(Debug, Clone)]
pub enum JobProgress {
    Progress(JobResult),
    Log(String),
    Done,
    Cancel,
}

impl JobResult {
    pub fn new(cmp: &[(usize, usize)]) -> Self {
        Self {
            time: 0,
            progress: 0,
            progress_all: u64::MAX,
            used: vec![false; cmp.len()],
            unsorted: [0; State::BITS as usize],
            log: String::new(),
        }
    }
    // Get unused comparators
    pub fn get_unused(&self) -> Vec<bool> {
        self.used.iter().map(|&u| !u).collect()
    }
    // Check if it is a sorting network
    pub fn is_sorting_network(&self) -> bool {
        self.unsorted.iter().all(|&f| f == 0)
    }
    // Get bitmap of positions where sorting is not done
    pub fn get_unsorted_bitmap(&self) -> [State; State::BITS as usize] {
        self.unsorted
    }
    // Get all pairs of positions where sorting is not done
    pub fn get_unsorted_allpairs(&self) -> Vec<(usize, usize)> {
        let mut unsorted = vec![];
        for i in 0..State::BITS as usize {
            let mut z = self.unsorted[i];
            while z != 0 {
                let j = z.trailing_zeros() as usize;
                unsorted.push((i, j));
                z &= z - 1;
            }
        }
        unsorted
    }
    // Get positions where sorting is not done
    pub fn get_unsorted_adjacent(&self) -> Vec<usize> {
        let mut unsorted = vec![];
        for i in 0..State::BITS as usize - 1 {
            if ((self.unsorted[i] >> i) & 2) != 0 {
                unsorted.push(i);
            }
        }
        unsorted
    }
}
pub struct JobResultFuture {
    progress_rx: mpsc::Receiver<JobProgress>,
    cancel_state: Arc<Mutex<bool>>,
}
impl JobResultFuture {
    pub fn recv_progress(&mut self) -> Result<JobProgress, mpsc::RecvError> {
        self.progress_rx.recv()
    }
    pub fn try_recv_progress(&mut self) -> Result<JobProgress, mpsc::TryRecvError> {
        self.progress_rx.try_recv()
    }
    pub fn cancel(&mut self) {
        *self.cancel_state.lock().unwrap() = true;
    }
}

// Fibonacci numbers: FIB1[0] = 1, FIB1[1] = 1, FIB1[i] = FIB1[i-1] + FIB1[i-2] (2 <= i <= State::BITS)
pub const FIB1: [State; (State::BITS + 1) as usize] = {
    let mut fib = [1; (State::BITS + 1) as usize];
    let mut i = 2;
    while i <= State::BITS as usize {
        fib[i] = fib[i - 1] + fib[i - 2];
        i += 1;
    }
    fib
};

#[derive(Debug, Clone, Copy)]
enum DsuBySizeElement {
    Size(usize),
    Parent(usize),
}
#[derive(Debug, Clone)]
// Disjoint Set Union (Union-Find) by Size
pub struct DsuBySize(Vec<DsuBySizeElement>);
impl DsuBySize {
    pub fn new(n: usize) -> Self {
        Self((0..n).map(|_| DsuBySizeElement::Size(1)).collect())
    }
    pub fn root_size(&mut self, u: usize) -> (usize, usize) {
        match self.0[u] {
            DsuBySizeElement::Size(size) => (u, size),
            DsuBySizeElement::Parent(v) if u == v => (u, 1),
            DsuBySizeElement::Parent(v) => {
                let (root, size) = self.root_size(v);
                self.0[u] = DsuBySizeElement::Parent(root);
                (root, size)
            }
        }
    }
    pub fn unite(&mut self, u: usize, v: usize) -> bool {
        let (u, size_u) = self.root_size(u);
        let (v, size_v) = self.root_size(v);
        if u == v {
            return false;
        }
        if size_u < size_v {
            self.0[u] = DsuBySizeElement::Parent(v);
            self.0[v] = DsuBySizeElement::Size(size_u + size_v);
        } else {
            self.0[v] = DsuBySizeElement::Parent(u);
            self.0[u] = DsuBySizeElement::Size(size_u + size_v);
        }
        true
    }
    pub fn root(&mut self, u: usize) -> usize {
        self.root_size(u).0
    }
    pub fn size(&mut self, u: usize) -> usize {
        self.root_size(u).1
    }
    pub fn equiv(&mut self, u: usize, v: usize) -> bool {
        self.root(u) == self.root(v)
    }
}

#[derive(Clone, Copy)]
struct CeEntry {
    cei: usize,
    a: usize,
    b: usize,
}
impl std::fmt::Debug for CeEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({},{},{})", self.cei, self.a, self.b)
    }
}
#[derive(Debug, Clone)]
enum VerifyJob {
    Cmp {
        root: usize,
        cmp_part: Vec<CeEntry>,
    },
    Combine {
        root_master: usize,
        root_slave: usize,
    },
}

fn verify_strategy(n: usize, cmp: &[(usize, usize)]) -> Vec<VerifyJob> {
    debug_assert!(2 <= n && n <= State::BITS as _);
    debug_assert!(cmp.iter().all(|&(a, b)| a < b && b < n));
    let mut cmp_layered = vec![false; cmp.len()];
    let mut cmp_skip = 0usize;
    let mut dsu = DsuBySize::new(n);
    let mut layers = vec![];
    while cmp_skip < cmp.len() {
        let mut node_avail = State::MAX >> (State::BITS - n as u32);
        let mut layer = (0..n).map(|_i| Vec::<CeEntry>::new()).collect::<Vec<_>>();
        let mut combine = (usize::MAX, 0, 0);
        for (i, &(a, b)) in cmp.iter().enumerate().skip(cmp_skip) {
            // if node_avail.count_ones() < 2 { break; }
            debug_assert_eq!(
                node_avail.count_ones() < 2,
                node_avail == 0 || node_avail.is_power_of_two()
            );
            if node_avail == 0 || node_avail.is_power_of_two() {
                break;
            }
            if cmp_layered[i] {
                continue;
            }
            let node_unavail = ((node_avail >> a) & (node_avail >> b) & 1) == 0;
            node_avail &= !((1 as State) << a) & !((1 as State) << b);
            if node_unavail {
                continue;
            }
            if dsu.equiv(a, b) {
                let root_a = dsu.root(a);
                layer[root_a].push(CeEntry { cei: i, a, b });
                cmp_layered[i] = true;
            } else {
                let (root_a, size_a) = dsu.root_size(a);
                let (root_b, size_b) = dsu.root_size(b);
                /*
                if combine.0 == usize::MAX || combine.1 > root_a {
                    combine = (size_a + size_b, root_a, root_b);
                }
                */
                combine = combine.min((size_a + size_b, root_a, root_b));
            }
        }
        if layer.iter().all(|v| v.is_empty()) {
            // Combine
            let (size, root_a, root_b) = combine;
            if size == usize::MAX {
                break;
            }
            let unite_result = dsu.unite(root_a, root_b);
            debug_assert!(unite_result);
            let root_master = dsu.root(root_a);
            let root_slave = root_a ^ root_b ^ root_master;
            layers.push(VerifyJob::Combine {
                root_master,
                root_slave,
            });
        } else {
            // Comparator
            for (root, ces) in layer.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
                layers.push(VerifyJob::Cmp {
                    root,
                    cmp_part: ces.clone(),
                });
            }
            cmp_skip += cmp_layered
                .iter()
                .skip(cmp_skip)
                .take_while(|&&f| f)
                .count();
        }
    }
    layers
}

fn execute_job_v2(
    pool: Arc<threadpool::ThreadPool>,
    progress_tx: mpsc::Sender<JobProgress>,
    cancel_state: Arc<Mutex<bool>>,
    n: usize,
    cmp: Arc<Vec<(usize, usize)>>,
) {
    let th = std::thread::spawn(move || {
        let begin_time = std::time::Instant::now();
        let mut result = JobResult::new(cmp.as_ref());
        result.progress_all = (cmp.len() as u64) + 1;
        progress_tx
            .send(JobProgress::Progress(result.clone()))
            .unwrap();
        let mut checked_cmp = vec![false; cmp.len()];
        let mut used_cmp = vec![false; cmp.len()];
        let mut states = (0..n)
            .map(|i| vec![((1 as State) << i, (1 as State) << i)])
            .collect::<Vec<_>>();
        let mut dsu = DsuBySize::new(n);
        for job in verify_strategy(n, &cmp) {
            {
                if *cancel_state.lock().unwrap() {
                    progress_tx.send(JobProgress::Cancel).unwrap();
                    return;
                }
            }
            match job {
                VerifyJob::Combine {
                    root_master,
                    root_slave,
                } => {
                    let begin_time_job = std::time::Instant::now();
                    debug_assert_eq!(dsu.root(root_master), root_master);
                    debug_assert_eq!(dsu.root(root_slave), root_slave);
                    let (conn_nodes_master, conn_nodes_slave) =
                        (dsu.size(root_master), dsu.size(root_slave));
                    let unite_result = dsu.unite(root_master, root_slave);
                    debug_assert!(unite_result);
                    debug_assert_eq!(dsu.root(root_master), root_master);
                    let conn_nodes_united = dsu.size(root_master);
                    let master_len = states[root_master].len();
                    let slave_len = states[root_slave].len();
                    let mut united_status = vec![(0, 0); master_len * slave_len];
                    united_status
                        .par_chunks_mut(master_len)
                        .zip(states[root_slave].par_iter())
                        .for_each(|(united_status_chunk, &(sz, so))| {
                            for (united_status, &(mz, mo)) in united_status_chunk
                                .iter_mut()
                                .zip(states[root_master].iter())
                            {
                                *united_status = (sz | mz, so | mo);
                            }
                        });
                    /*
                    let mut united_status =
                        Vec::with_capacity(states[root_master].len() * states[root_slave].len());
                    for &(sz, so) in states[root_slave].iter() {
                        for &(mz, mo) in states[root_master].iter() {
                            united_status.push((sz | mz, so | mo));
                        }
                    }
                    */
                    let united_len = united_status.len();
                    states[root_slave] = vec![];
                    states[root_master] = united_status;
                    let elapsed_time = begin_time_job.elapsed().as_millis() as u64;
                    let log = format!(
                        "Combining, conn: {conn_nodes_master}+{conn_nodes_slave}=>{conn_nodes_united}, root: ({root_master},{root_slave}), len: {master_len}*{slave_len}=>{united_len}, time: {elapsed_time}ms"
                    );
                    eprintln!("{}", log);
                    progress_tx.send(JobProgress::Log(log)).unwrap();
                }
                VerifyJob::Cmp { root, cmp_part } => {
                    let begin_time_job = std::time::Instant::now();
                    let mut elapsed_times = vec![];
                    debug_assert_eq!(dsu.root(root), root);
                    debug_assert!(
                        cmp_part
                            .iter()
                            .all(|&CeEntry { cei: _, a, b }| dsu.equiv(root, a)
                                && dsu.equiv(root, b))
                    );
                    let conn_nodes = dsu.size(root);
                    let pre_len = states[root].len();
                    let mut stack =
                        Vec::<(usize, State, State)>::with_capacity(states[root].len() + n);
                    let mut x = 0;
                    let states_root = &mut states[root];
                    let (par_unused_cmp, par_extend_states): (Vec<_>, Vec<_>) = states_root
                        .par_chunks_mut(65536)
                        .map_with(cancel_state.clone(), |cancel_state, states_chunk| {
                            let mut stack =
                                Vec::<(usize, State, State)>::with_capacity(states_chunk.len() + n);
                            let mut extend_states = Vec::new();
                            let mut used_cmp_local = vec![false; cmp.len()];
                            {
                                if *cancel_state.lock().unwrap() {
                                    return (used_cmp_local, extend_states);
                                }
                            }

                            for st in states_chunk.iter_mut() {
                                let (mut z, mut o) = *st;
                                for (i, &CeEntry { cei, a, b }) in cmp_part.iter().enumerate() {
                                    if 1 & (o >> a) & (z >> b) == 0 {
                                        continue;
                                    } else if 1 & (z >> a) & (o >> b) == 0 {
                                        used_cmp_local[cei] = true;
                                        let (xz, xo) =
                                            (((z >> a) ^ (z >> b)) & 1, ((o >> a) ^ (o >> b)) & 1);
                                        z ^= xz << a | xz << b;
                                        o ^= xo << a | xo << b;
                                    } else {
                                        used_cmp_local[cei] = true;
                                        stack.push((
                                            i + 1,
                                            z,
                                            o ^ ((1 as State) << a) ^ ((1 as State) << b),
                                        ));
                                        z ^= (1 as State) << b;
                                    }
                                }
                                *st = (z, o);
                            }
                            while let Some((mut i, mut z, mut o)) = stack.pop() {
                                while let Some(&CeEntry { cei, a, b }) = cmp_part.get(i) {
                                    i += 1;
                                    if (o >> a) & 1 == 0 || (z >> b) & 1 == 0 {
                                        continue;
                                    } else if (z >> a) & 1 == 0 || (o >> b) & 1 == 0 {
                                        used_cmp_local[cei] = true;
                                        let (xz, xo) =
                                            (((z >> a) ^ (z >> b)) & 1, ((o >> a) ^ (o >> b)) & 1);
                                        z ^= xz << a | xz << b;
                                        o ^= xo << a | xo << b;
                                    } else {
                                        used_cmp_local[cei] = true;
                                        stack.push((
                                            i,
                                            z,
                                            o ^ ((1 as State) << a) ^ ((1 as State) << b),
                                        ));
                                        z ^= (1 as State) << b;
                                    }
                                }
                                extend_states.push((z, o));
                            }
                            extend_states.sort_unstable();
                            extend_states.dedup();
                            (used_cmp_local, extend_states)
                        })
                        .unzip();
                    {
                        if *cancel_state.lock().unwrap() {
                            progress_tx.send(JobProgress::Cancel).unwrap();
                            return;
                        }
                    }
                    elapsed_times.push(("states", begin_time_job.elapsed().as_millis()));
                    for unused_part in par_unused_cmp.iter() {
                        for (uroot, &ue) in used_cmp.iter_mut().zip(unused_part.iter()) {
                            *uroot |= ue;
                        }
                    }
                    elapsed_times.push(("unused", begin_time_job.elapsed().as_millis()));
                    let ext_len = par_extend_states.iter().map(|v| v.len()).sum();
                    states_root.reserve(ext_len);
                    for extend_states in par_extend_states {
                        states_root.extend(extend_states);
                    }
                    {
                        if *cancel_state.lock().unwrap() {
                            progress_tx.send(JobProgress::Cancel).unwrap();
                            return;
                        }
                    }
                    elapsed_times.push(("extend", begin_time_job.elapsed().as_millis()));
                    let gen_len = states_root.len();
                    // dedupulicate
                    if ext_len > 0 {
                        states_root.par_sort_unstable();
                        //states_root.sort_unstable();
                        {
                            if *cancel_state.lock().unwrap() {
                                progress_tx.send(JobProgress::Cancel).unwrap();
                                return;
                            }
                        }
                        elapsed_times.push(("sort", begin_time_job.elapsed().as_millis()));
                        states_root.dedup();
                        {
                            if *cancel_state.lock().unwrap() {
                                progress_tx.send(JobProgress::Cancel).unwrap();
                                return;
                            }
                        }
                        elapsed_times.push(("dedup", begin_time_job.elapsed().as_millis()));
                    }
                    let dedup_len = states_root.len();
                    // send result
                    result.used = used_cmp.clone();
                    for &CeEntry { cei, a: _, b: _ } in cmp_part.iter() {
                        result.progress += 1;
                        checked_cmp[cei] = true;
                    }
                    {
                        if *cancel_state.lock().unwrap() {
                            progress_tx.send(JobProgress::Cancel).unwrap();
                            return;
                        }
                    }
                    let log = format!(
                        "AppliedCE, conn: {conn_nodes}, root: {root}, len: {pre_len}=>{gen_len}=>{dedup_len}, cmp: {cmp_part:?}, time: {elapsed_times:?}ms"
                    );
                    eprintln!("{}", log);
                    result.log = log.clone();
                    result.time = begin_time.elapsed().as_millis() as u64;
                    progress_tx
                        .send(JobProgress::Progress(result.clone()))
                        .unwrap();
                    elapsed_times.push(("send", begin_time_job.elapsed().as_millis()));
                }
            }
        }
        fn check_unsorted(unsorted: &mut [State; State::BITS as _], z: State, o: State) {
            let (q, rz, mut ro) = (z | o, z, o);
            while ro != 0 {
                let i = ro.trailing_zeros() as usize;
                unsorted[i] |= rz & ((State::MAX << 1) << i);
                ro &= ro - 1;
            }
        }
        for states_par_root in states.iter() {
            let unsorted = &mut result.unsorted;
            let n_mask = State::MAX >> (State::BITS - n as u32);
            let q_mask = states_par_root.first().map(|&(z, o)| z | o).unwrap_or(0);
            let nq_mask = n_mask ^ q_mask;
            check_unsorted(unsorted, q_mask, nq_mask);
            check_unsorted(unsorted, nq_mask, q_mask);
            for &(z, o) in states_par_root.iter() {
                check_unsorted(unsorted, z, o);
            }
        }
        result.progress = result.progress_all;
        result.time = begin_time.elapsed().as_millis() as u64;
        let log = format!(
            "Finished, progress: {progress}/{progress_all}, unused_cmp: {cmp_unused}/{cmp_count}, unsorted: {unsorted}/{unsorted_all} ({unsorted_d}/{unsorted_d_all}), time: {time}ms",
            progress = result.progress,
            progress_all = result.progress_all,
            cmp_unused = result.get_unused().iter().filter(|&&u| u).count(),
            cmp_count = cmp.len(),
            unsorted = result.unsorted.iter().map(|x| x.count_ones()).sum::<u32>(),
            unsorted_all = (n * (n - 1) / 2),
            unsorted_d = result
                .unsorted
                .iter()
                .enumerate()
                .filter(|&(i, &x)| ((x >> i) & 2) != 0)
                .count(),
            unsorted_d_all = n - 1,
            time = result.time,
        );
        result.log = log.clone();
        progress_tx
            .send(JobProgress::Progress(result.clone()))
            .unwrap();
        progress_tx.send(JobProgress::Done).unwrap();
    });
}

pub fn is_sorting_network_future_v2(
    pool: Arc<threadpool::ThreadPool>,
    n: usize,
    cmp: Arc<Vec<(usize, usize)>>,
) -> JobResultFuture {
    let (progress_tx, progress_rx) = mpsc::channel::<JobProgress>();
    let cancel_state = Arc::new(Mutex::new(false));
    execute_job_v2(pool, progress_tx, cancel_state, n, Arc::clone(&cmp));
    JobResultFuture {
        progress_rx,
        cancel_state: Arc::new(Mutex::new(false)),
    }
}

pub fn parse_network(net: &str) -> Result<(usize, usize, Vec<(usize, usize)>), String> {
    let mut lines = net.lines();
    let mut w = lines
        .next()
        .ok_or_else(|| "empty input".to_string())?
        .split_ascii_whitespace();
    let n: usize = w
        .next()
        .ok_or_else(|| "missing n".to_string())?
        .parse()
        .map_err(|_| "parseint failed n".to_string())?;
    if n < 2 || n > 64 {
        return Err("invalid n".to_string());
    }
    let m: usize = w
        .next()
        .ok_or_else(|| "missing m".to_string())?
        .parse()
        .map_err(|_| "parseint failed m".to_string())?;
    let a = lines
        .next()
        .ok_or_else(|| "missing a".to_string())?
        .split_ascii_whitespace()
        .map(|x| x.parse().map_err(|_| "parseint failed a".to_string()))
        .collect::<Result<Vec<usize>, String>>()?;
    let b = lines
        .next()
        .ok_or_else(|| "missing a".to_string())?
        .split_ascii_whitespace()
        .map(|x| x.parse().map_err(|_| "parseint failed b".to_string()))
        .collect::<Result<Vec<usize>, String>>()?;
    if a.len() != m
        || b.len() != m
        || a.iter().any(|&x| x < 1 || x > n)
        || b.iter().any(|&x| x < 1 || x > n)
    {
        return Err("invalid input".to_string());
    }
    let cmp = a
        .iter()
        .zip(b.iter())
        .map(|(&a, &b)| (a - 1, b - 1))
        .collect::<Vec<_>>();
    if cmp.iter().any(|&(a, b)| a >= b) {
        return Err("invalid comparators".to_string());
    }

    Ok((n, m, cmp))
}

#[derive(Clone, Debug)]
pub struct SvgPos {
    pub n: usize,
    pub d: usize,
    pub cmp: Vec<(usize, usize)>,
    pub width: usize,
    pub height: usize,
    pub x_pos: Vec<usize>,
}
impl SvgPos {
    pub fn new(n: usize, cmp: &[(usize, usize)]) -> Self {
        gen_svg_pos(n, cmp)
    }
}

pub fn gen_svg_pos(n: usize, cmp: &[(usize, usize)]) -> SvgPos {
    let x_scale = 35;
    let x_scale_thin = 11;
    let y_scale = 20;

    let mut width = x_scale * 2 + x_scale_thin * cmp.len().saturating_sub(1);
    let height = y_scale * (n + 1);
    let mut x_pos = (0..cmp.len())
        .map(|i| i * x_scale_thin + x_scale)
        .collect::<Vec<_>>();
    let mut d = 0;
    if cmp.iter().any(|&(a, b)| a >= n || b >= n || a >= b) {
        return SvgPos {
            n,
            d,
            cmp: cmp.to_vec(),
            width,
            height,
            x_pos,
        };
    }
    let mut w = x_scale;
    let mut remain_cmp = cmp.iter().copied().enumerate().collect::<Vec<_>>();
    while !remain_cmp.is_empty() {
        d += 1;
        let mut used = vec![false; n];
        let mut curr_cmp = Vec::with_capacity(n / 2);
        let mut next_cmp = Vec::with_capacity(remain_cmp.len());
        let mut f = true;
        for &(i, (a, b)) in remain_cmp.iter() {
            if f && !used[a] && !used[b] {
                curr_cmp.push((i, (a, b)));
            } else {
                f = false;
                next_cmp.push((i, (a, b)));
            }
            used[a] = true;
            used[b] = true;
        }
        //curr_cmp.sort_by_key(|&(i, (a, _))| (a, i));
        let mut gfill = Vec::<Vec<bool>>::new();
        'a: for &(i, (a, b)) in curr_cmp.iter() {
            for (j, l) in gfill.iter_mut().enumerate() {
                if l[a..=b].iter().any(|&f| f) {
                    continue;
                }
                x_pos[i] = w + x_scale_thin * j;
                l[a..=b].fill(true);
                continue 'a;
            }
            x_pos[i] = w + x_scale_thin * gfill.len();
            gfill.push(vec![false; n]);
            gfill.last_mut().unwrap()[a..=b].fill(true);
        }
        w += gfill.len().saturating_sub(1) * x_scale_thin + x_scale;
        remain_cmp = next_cmp;
    }

    width = w;

    SvgPos {
        n,
        d,
        cmp: cmp.to_vec(),
        width,
        height,
        x_pos,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SortingNetworkSvg {
    width: usize,
    height: usize,
    path_nodes: String,
    path_cmp_normal: String,
    path_cmp_unused: String,
    path_nodes_unknown: String,
    path_nodes_unsorted: String,
}

pub fn gen_svg(pos: &SvgPos, result: &JobResult) -> SortingNetworkSvg {
    let x_scale = 35;
    let y_scale = 20;
    //let line_width = 1;
    let r = 3;
    let r2 = r * 2;

    let mut path_nodes = String::new();
    let mut path_cmp_normal = String::new();
    let mut path_cmp_unused = String::new();
    let mut path_nodes_unknown = String::new();
    let mut path_nodes_unsorted = String::new();

    for (i, (&(a, b), &x)) in pos.cmp.iter().zip(pos.x_pos.iter()).enumerate() {
        let y1 = y_scale * (a + 1) + r;
        let yd = y_scale * (b - a) - r2;
        let path = format!(
            "M{x} {y1}a{r} {r} 0 1 1 0-{r2}a{r} {r} 0 1 1 0 {r2}v{yd}a{r} {r} 0 1 1 0 {r2}a{r} {r} 0 1 1 0-{r2}z",
        );
        if result.used[i] {
            path_cmp_normal.push_str(&path);
        } else {
            path_cmp_unused.push_str(&path);
        }
    }
    let unsort_x = pos.width - x_scale / 2;
    for (i, &f) in result.unsorted[..(pos.n.saturating_sub(1))]
        .iter()
        .enumerate()
    {
        let y = y_scale * (2 * i + 3) / 2 + r;
        let path = format!("M{unsort_x} {y}a{r} {r} 0 1 1 0-{r2}a{r} {r} 0 1 1 0 {r2}z");
        if ((f >> 1) >> i) & 1 != 0 {
            path_nodes_unsorted.push_str(&path);
        } else if result.progress < result.progress_all {
            path_nodes_unknown.push_str(&path);
        }
    }

    for i in 0..pos.n {
        path_nodes.push_str(&format!(
            "M0 {y}h{width}",
            y = y_scale * (i + 1),
            width = pos.width
        ));
    }

    SortingNetworkSvg {
        width: pos.width,
        height: pos.height,
        path_nodes,
        path_cmp_normal,
        path_cmp_unused,
        path_nodes_unknown,
        path_nodes_unsorted,
    }
}

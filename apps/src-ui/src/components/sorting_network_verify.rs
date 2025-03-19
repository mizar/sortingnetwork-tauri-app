use core::f64;

use crate::components::sorting_network_opts;
use futures::stream::StreamExt;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use thaw::*;

#[derive(Serialize, Deserialize)]
struct TaskParams {
    id: u32,
    net: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProgressUpdate {
    n: usize,
    l: usize,
    d: usize,
    max_branches: u64,
    branches: u64,
    used: Vec<bool>,
    unsorted: Vec<Vec<bool>>,
    svg: SortingNetworkSvg,
    time: u64,
    log: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SortingNetworkSvg {
    width: usize,
    height: usize,
    path_nodes: String,
    path_cmp_normal: String,
    path_cmp_unused: String,
    path_nodes_unknown: String,
    path_nodes_unsorted: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum EmitType {
    Progress(ProgressUpdate),
    Log(String),
    Error(String),
    CancelRecv,
    Cencelled,
    Done,
}

fn gen_procon(n: usize, cmp: &[(usize, usize)]) -> String {
    let mut procon = String::new();
    procon.push_str(&format!("{} {}\n", n, cmp.len()));
    procon.push_str(
        &cmp.iter()
            .map(|&(i, _)| (i + 1).to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    procon.push_str("\n");
    procon.push_str(
        &cmp.iter()
            .map(|&(_, j)| (j + 1).to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    procon.push_str("\n");
    procon
}

fn gen_bubble_max(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    for p in 0..(2 * n - 3) {
        for i in ((p & 1)..((p + 1).min(2 * n - p - 2))).step_by(2) {
            cmp.push((i, i + 1));
        }
    }
    cmp
}
fn gen_bubble_min(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    for p in 0..(2 * n - 3) {
        for i in (n.abs_diff(p + 2)..(n - 1)).step_by(2) {
            cmp.push((i, i + 1));
        }
    }
    cmp
}
fn gen_oddeven(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    for p in 0..n {
        for i in ((p & 1)..(n - 1)).step_by(2) {
            cmp.push((i, i + 1));
        }
    }
    cmp
}
fn triangular_indices(n: usize) -> (usize, usize) {
    let r = ((8 * n + 1).isqrt() - 1) / 2;
    (r, n - r * (r + 1) / 2)
}
fn gen_bitonic(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    let r = (n).next_power_of_two().ilog2() as usize;
    for d in 0..(r * (r + 1) / 2) {
        let (m, p) = triangular_indices(d);
        for i in 0..n {
            let j = if p == 0 {
                i ^ ((2 << m) - 1)
            } else {
                i ^ (1 << (m - p))
            };
            if i < j && j < n {
                cmp.push((i, j));
            }
        }
    }
    cmp
}
fn gen_batcher(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    let r = (n).next_power_of_two().ilog2() as usize;
    for d in 0..(r * (r + 1) / 2) {
        let (m, p) = triangular_indices(d);
        for i in 0..n {
            let j = if p == 0 {
                i ^ (1 << m)
            } else {
                let (scale, boxmask) = (m - p, (2usize << p) - 1);
                let sn = (i >> scale) & boxmask;
                if sn == 0 || sn == boxmask {
                    i
                } else if (sn & 1) == 0 {
                    i - (1 << scale)
                } else {
                    i + (1 << scale)
                }
            };
            if i < j && j < n {
                cmp.push((i, j));
            }
        }
    }
    cmp
}
fn gen_pairwise(n: usize) -> Vec<(usize, usize)> {
    assert!(2 <= n && n <= 64);
    let mut cmp = Vec::new();
    let r = (n).next_power_of_two().ilog2() as usize;
    for d in 0..(r * (r + 1) / 2) {
        let (m, p) = if d < r {
            (0, d)
        } else {
            let (tm, tp) = triangular_indices(d - r);
            (tm + 1, tp)
        };
        for i in 0..n {
            let j = if m == 0 {
                i ^ (1 << p)
            } else {
                let dj = (1 << (r - p - 1)) - (1 << (r - m - 1));
                if ((i >> (r - m - 1)) & 1) == 0 {
                    if i >= dj { i - dj } else { i }
                } else {
                    i + dj
                }
            };
            if i < j && j < n {
                cmp.push((i, j));
            }
        }
    }
    cmp
}

#[component]
pub fn SortingNetworkVerify() -> impl IntoView {
    let placeholder = "2 1\n1\n2\n";
    let taskid = RwSignal::new(0u32);
    let net = RwSignal::new(String::new());
    let netresult = RwSignal::new(String::new());
    let progress_value = RwSignal::new(0f64);
    let progress_text = RwSignal::new("".to_string());
    let n_value = RwSignal::new(32usize);
    let svg_width = RwSignal::new(0usize);
    let svg_height = RwSignal::new(0usize);
    let svg_view_box = RwSignal::new("0 0 0 0".to_string());
    let svg_path_nodes = RwSignal::new(String::new());
    let svg_path_cmp_normal = RwSignal::new(String::new());
    let svg_path_cmp_unused = RwSignal::new(String::new());
    let svg_path_nodes_unknown = RwSignal::new(String::new());
    let svg_path_nodes_unsorted = RwSignal::new(String::new());
    let select_value = RwSignal::new("Default".to_string());

    let on_click = move |_: leptos::ev::MouseEvent| {
        netresult.set("*in progress*".to_string());
        taskid.set(taskid.get_untracked().wrapping_add(1));
        leptos::task::spawn_local(async move {
            let result: String = tauri_sys::core::invoke(
                "sorting_network_verify",
                TaskParams {
                    id: taskid.get_untracked(),
                    net: net.get_untracked(),
                },
            )
            .await;
            log::info!("result: {:?}", result);
        });
    };

    let ev_bubble_max = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_bubble_max(n_value.get_untracked()),
        ));
        select_value.set("BubbleMax".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_bubble_min = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_bubble_min(n_value.get_untracked()),
        ));
        select_value.set("BubbleMin".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_oddeven = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_oddeven(n_value.get_untracked()),
        ));
        select_value.set("OddEven".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_bitonic = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_bitonic(n_value.get_untracked()),
        ));
        select_value.set("Bitonic".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_batcher = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_batcher(n_value.get_untracked()),
        ));
        select_value.set("Batcher".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_pairwise = move |_| {
        net.set(gen_procon(
            n_value.get_untracked(),
            &gen_pairwise(n_value.get_untracked()),
        ));
        select_value.set("Pairwise".to_string());
        on_click(leptos::ev::MouseEvent::new("click").unwrap());
    };
    let ev_select = move |_| {
        for &(id, n, _m, _d, a, b) in sorting_network_opts::OPT_NET.iter() {
            if id == select_value.get_untracked() {
                net.set(gen_procon(
                    n as usize,
                    &a.iter()
                        .zip(b.iter())
                        .map(|(&x, &y)| (x as usize, y as usize))
                        .collect::<Vec<_>>(),
                ));
                on_click(leptos::ev::MouseEvent::new("click").unwrap());
                break;
            }
        }
    };
    let ta_ref = NodeRef::<leptos::html::Textarea>::new();

    leptos::task::spawn_local(async move {
        let mut listener = tauri_sys::event::listen::<(u32, EmitType)>("checkprogress")
            .await
            .unwrap();

        while let Some(event) = listener.next().await {
            match event.payload {
                (_id, EmitType::Progress(x)) => {
                    log::info!("progress: {:?}", x);
                    progress_value.set((x.branches as f64) / (x.max_branches.max(1) as f64));
                    progress_text.set(format!(
                        "n: {n}, l: {l}, d: {d}, progress: {percent}%, elapsed: {elapsed:.3}sec, unused_cmp {unused}/{unused_all}, unsorted {unsorted}/{unsorted_all} ({unsorted_d}/{unsorted_d_all})",
                        n = x.n,
                        l = x.l,
                        d = x.d,
                        percent = (x.branches * 100) / x.max_branches.max(1),
                        elapsed = x.time as f64 / 1000.0,
                        unused = x.used.iter().filter(|&&x| !x).count(),
                        unused_all = x.used.len(),
                        unsorted = x.unsorted.iter().flatten().filter(|x| **x).count(),
                        unsorted_all = x.n * (x.n - 1) / 2,
                        unsorted_d = x.unsorted.iter().enumerate().filter(|&(i, x)| x.get(i + 1).copied().unwrap_or(false)).count(),
                        unsorted_d_all = x.n - 1,
                    ));
                    netresult.set(format!(
                        "{prev}\n{e}",
                        prev = netresult.get_untracked(),
                        e = x.log
                    ));
                    svg_width.set(x.svg.width);
                    svg_height.set(x.svg.height);
                    svg_view_box.set(format!("0 0 {} {}", x.svg.width, x.svg.height));
                    svg_path_nodes.set(x.svg.path_nodes);
                    svg_path_cmp_normal.set(x.svg.path_cmp_normal);
                    svg_path_cmp_unused.set(x.svg.path_cmp_unused);
                    svg_path_nodes_unknown.set(x.svg.path_nodes_unknown);
                    svg_path_nodes_unsorted.set(x.svg.path_nodes_unsorted);
                    if x.branches == x.max_branches {
                        let yes_no = x.unsorted.iter().flatten().all(|&x| !x);
                        let unused_indexes = x
                            .used
                            .iter()
                            .enumerate()
                            .filter_map(|(i, &x)| if !x { Some(i + 1) } else { None })
                            .collect::<Vec<_>>();
                        let unsorted_indexes = (1..64)
                            .filter_map(|i| if x.unsorted[i - 1][i] { Some(i) } else { None })
                            .collect::<Vec<_>>();
                        netresult.set(format!(
                            "{log}\n---\n{yes_no}\n{len}\n{indexes}\n",
                            log = netresult.get_untracked(),
                            yes_no = if yes_no { "Yes" } else { "No" },
                            len = if yes_no {
                                unused_indexes.len()
                            } else {
                                unsorted_indexes.len()
                            },
                            indexes = (if yes_no {
                                unused_indexes
                            } else {
                                unsorted_indexes
                            })
                            .iter()
                            .map(|&x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                        ));
                    }
                    leptos::task::spawn_local(async move {
                        if let Some(ta) = ta_ref.get() {
                            ta.set_scroll_top(ta.scroll_height());
                        }
                    });
                }
                (id, EmitType::Done) => {
                    log::info!("{id}: done");
                    leptos::task::spawn_local(async move {
                        if let Some(ta) = ta_ref.get() {
                            ta.set_scroll_top(ta.scroll_height());
                        }
                    });
                }
                (id, EmitType::Error(e)) => {
                    netresult.set(format!("error: {e:?}"));
                    log::error!("{id}: error: {e:?}");
                }
                (_id, EmitType::Log(e)) => {
                    netresult.set(format!("{prev}\n{e}", prev = netresult.get_untracked()));
                    leptos::task::spawn_local(async move {
                        if let Some(ta) = ta_ref.get() {
                            ta.set_scroll_top(ta.scroll_height());
                        }
                    });
                }
                (id, EmitType::Cencelled) => {
                    netresult.set("cancelled".to_string());
                    log::info!("{id}: cancelled");
                }
                (id, EmitType::CancelRecv) => {
                    log::info!("{id}: cancel recv");
                }
            }
        }
    });

    //             N:<input type="number" min=2 max=64 placeholder="N" prop:value=move || n_value.get() on:input:target=move |ev| n_value.set(ev.target().value().parse().unwrap_or(2)) />
    //             N:<SpinButton<usize> min=2 max=64 step_page=1 value=n_value />

    view! {
        <ConfigProvider>
            <p>
            N:<input type="number" min=2 max=64 placeholder="N" prop:value=move || n_value.get() on:input:target=move |ev| n_value.set(ev.target().value().parse().unwrap_or(2)) />
            <Button appearance=ButtonAppearance::Secondary on_click=ev_bubble_max>"BubbleMax"</Button>
            <Button appearance=ButtonAppearance::Secondary on_click=ev_bubble_min>"BubbleMin"</Button>
            <Button appearance=ButtonAppearance::Secondary on_click=ev_oddeven>"OddEven"</Button>
            <Button appearance=ButtonAppearance::Secondary on_click=ev_bitonic>"Bitonic"</Button>
            <Button appearance=ButtonAppearance::Secondary on_click=ev_batcher>"Batcher"</Button>
            <Button appearance=ButtonAppearance::Secondary on_click=ev_pairwise>"Pairwise"</Button>
            </p>
            <div>
            <Select value=select_value on:change=ev_select>
                <option></option>
                {
                    sorting_network_opts::OPT_NET.iter().map(|(id, _, _, _, _, _)| {
                        view! {
                            <option>{*id}</option>
                        }
                    }).collect_view()
                }
            </Select>
            </div>
            <div>
            <ProgressBar value=progress_value />
            </div>
            <div class="row">
                <p>{progress_text}</p>
                //<p><progress max=100 value=progress_value></progress></p>
                <p><Button appearance=ButtonAppearance::Secondary on_click>"Verify"</Button></p>
            </div>
            <textarea rows=4 placeholder=placeholder prop:value=move || net.get() on:input:target=move |ev| net.set(ev.target().value()) class="network"></textarea>
            <textarea rows=4 prop:value=move || netresult.get() class="network" readonly node_ref=ta_ref></textarea>
            <div class="network">
                <svg viewBox=svg_view_box class="network">
                    <rect x=0 y=0 width=svg_width height=svg_height fill="white" />
                    <path d=svg_path_nodes stroke-width=1 stroke="rgb(0,0,0)" />
                    <path d=svg_path_cmp_normal stroke-width=1 stroke="rgb(0,0,0)" fill="rgb(0,0,0)" />
                    <path d=svg_path_cmp_unused stroke-width=1 stroke="rgb(255,0,0)" fill="rgb(255,0,0)" />
                    <path d=svg_path_nodes_unknown fill="rgba(255,0,0,0.2)" />
                    <path d=svg_path_nodes_unsorted fill="rgb(255,0,0)" />
                </svg>
            </div>
        </ConfigProvider>
    }
}

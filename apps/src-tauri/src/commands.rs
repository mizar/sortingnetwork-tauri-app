use crate::{
    SortingNetworkVerifyId,
    /*
    sorting_network_check::{
        gen_svg, gen_svg_pos, is_sorting_network_future_v1, parse_network, SortingNetworkSvg, FIB1,
    },
    */
    sorting_network_check_v2::{
        JobProgress, SortingNetworkSvg, SvgPos, gen_svg, is_sorting_network_future_v2,
        parse_network, JobResult,
    },
    threadpool::ThreadPool,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Deserialize)]
pub struct GreetArgs {
    name: String,
}

#[derive(Serialize, Deserialize)]
pub struct TaskParams {
    id: u32,
    net: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProgressUpdate {
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
pub enum EmitType {
    Progress(ProgressUpdate),
    Log(String),
    Error(String),
    CancelRecv,
    Cencelled,
    Done,
}

/*
#[tauri::command]
pub fn greet(name: String) -> String {
    println!("invoked: {name}");
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub async fn trigger_backend_event(app: AppHandle) {
    let emit = |x| app.emit("backend", x).unwrap();

    for i in 1..=1000 {
        emit(Some(i));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    emit(None);
}
*/

#[tauri::command]
pub async fn sorting_network_verify(id: u32, net: String, app: AppHandle) -> String {
    let emit = |x| {
        app.emit::<(u32, EmitType)>("checkprogress", (id, x))
            .unwrap()
    };
    let get_pool = || app.state::<Arc<ThreadPool>>();
    {
        app.state::<Mutex<SortingNetworkVerifyId>>()
            .lock()
            .unwrap()
            .set(id);
    }
    let get_id = || {
        app.state::<Mutex<SortingNetworkVerifyId>>()
            .lock()
            .unwrap()
            .get()
    };
    match parse_network(&net) {
        Ok((n, l, cmp)) => {
            let pos = SvgPos::new(n, &cmp);
            let pool = Arc::clone(&get_pool());
            let mut last_progress = JobResult::new(&cmp);
            let svg_default = SortingNetworkSvg::default();
            let mut future = is_sorting_network_future_v2(pool, n, Arc::new(cmp.clone()));
            loop {
                if id != get_id() {
                    future.cancel();
                    emit(EmitType::CancelRecv);
                    return "cancelled".to_string();
                }
                let mut progress_update = match future.recv_progress() {
                    Ok(JobProgress::Progress(progress)) => {
                        last_progress = progress.clone();
                        EmitType::Progress(ProgressUpdate {
                            n,
                            l,
                            d: pos.d,
                            max_branches: progress.progress_all as _,
                            branches: progress.progress as _,
                            used: progress.used,
                            unsorted: progress
                                .unsorted
                                .iter()
                                .map(|&x| {
                                    (0..crate::sorting_network_check_v2::State::BITS)
                                        .map(|i| (x >> i) & 1 != 0)
                                        .collect()
                                })
                                .collect(),
                            svg: svg_default.clone(),
                            time: progress.time,
                            log: progress.log,
                        })
                    }
                    Ok(JobProgress::Log(log)) => {
                        EmitType::Log(log)
                    }
                    Ok(JobProgress::Cancel) => {
                        emit(EmitType::Cencelled);
                        return "cancelled".to_string();
                    }
                    Ok(JobProgress::Done) => {
                        emit(EmitType::Done);
                        return "done verify".to_string();
                    }
                    Err(e) => {
                        let msg = format!("error: {}", e);
                        emit(EmitType::Error(msg.clone()));
                        return msg;
                    }
                };
                // Reduce the frequency of notifications to the frontend
                loop {
                    match future.try_recv_progress() {
                        Ok(JobProgress::Progress(progress)) => {
                            let prev_log = match progress_update {
                                EmitType::Progress(ProgressUpdate { log, .. }) => log + "\n",
                                EmitType::Log(log) => log + "\n",
                                _ => String::new(),
                            };
                            last_progress = progress.clone();
                            progress_update = EmitType::Progress(ProgressUpdate {
                                n,
                                l,
                                d: pos.d,
                                max_branches: progress.progress_all as _,
                                branches: progress.progress as _,
                                used: progress.used,
                                unsorted: progress
                                    .unsorted
                                    .iter()
                                    .map(|&x| {
                                        (0..crate::sorting_network_check_v2::State::BITS)
                                            .map(|i| (x >> i) & 1 != 0)
                                            .collect()
                                    })
                                    .collect(),
                                svg: svg_default.clone(),
                                time: progress.time,
                                log: prev_log + &progress.log,
                            });
                        }
                        Ok(JobProgress::Log(log)) => {
                            progress_update = match progress_update {
                                EmitType::Progress(pu) => {
                                    EmitType::Progress(ProgressUpdate {
                                        log: pu.log + "\n" + &log,
                                        ..pu
                                    })
                                }
                                EmitType::Log(prev_log) => EmitType::Log(prev_log + "\n" + &log),
                                _ => EmitType::Log(log),
                            };
                        }
                        Ok(JobProgress::Cancel) => {
                            emit(EmitType::Cencelled);
                            return "cancelled".to_string();
                        }
                        Ok(JobProgress::Done) => {
                            // Update the SVG
                            if let EmitType::Progress(pp) = progress_update {
                                progress_update = EmitType::Progress(ProgressUpdate {
                                    svg: gen_svg(&pos, &last_progress),
                                    ..pp
                                });
                            };
                            emit(progress_update);
                            emit(EmitType::Done);
                            return "done verify".to_string();
                        }
                        Err(_) => break,
                    }
                }
                // Update the SVG
                if let EmitType::Progress(pp) = progress_update {
                    progress_update = EmitType::Progress(ProgressUpdate {
                        svg: gen_svg(&pos, &last_progress),
                        ..pp
                    });
                };
                if id != get_id() {
                    future.cancel();
                    //emit(Err("cancelled".to_string()));
                    emit(EmitType::CancelRecv);
                    eprintln!("cancelled");
                    return "cancelled".to_string();
                }
                emit(progress_update);
                /*
                match progress {
                    Ok(JobProgress::Progress(progress)) => {
                        let svg = gen_svg(&pos, &progress);
                        emit(EmitType::Progress(ProgressUpdate {
                            n,
                            l,
                            d: pos.d,
                            max_branches: progress.progress_all as _,
                            branches: progress.progress as _,
                            used: progress.used,
                            unsorted: progress
                                .unsorted
                                .iter()
                                .map(|&x| {
                                    (0..crate::sorting_network_check_v2::State::BITS)
                                        .map(|i| (x >> i) & 1 != 0)
                                        .collect()
                                })
                                .collect(),
                            svg,
                            time: progress.time,
                            log: progress.log,
                        }));
                    }
                    Ok(JobProgress::Log(log)) => {
                        emit(EmitType::Log(log));
                    }
                    Ok(JobProgress::Cancel) => {
                        emit(EmitType::Cencelled);
                        return "cancelled".to_string();
                    }
                    Ok(JobProgress::Done) => {
                        emit(EmitType::Done);
                        return "done verify".to_string();
                    }
                    Err(e) => {
                        let msg = format!("error: {}", e);
                        emit(EmitType::Error(msg.clone()));
                        return msg;
                    }
                }
                */
                if id != get_id() {
                    future.cancel();
                    //emit(Err("cancelled".to_string()));
                    emit(EmitType::CancelRecv);
                    eprintln!("cancelled");
                    return "cancelled".to_string();
                }
            }
            /*
            let mut future = is_sorting_network_future_v1(pool, n, Arc::new(cmp.clone()));
            let fib1_n = FIB1[n];
            let mut next_progress = 0;
            let mut prev_used = 0;
            let mut prev_unsorted = 0;
            loop {
                if id != get_id() {
                    future.cancel();
                    emit(Err("cancelled".to_string()));
                    return "cancelled".to_string();
                }
                let progress = future.recv_progress();
                if id != get_id() {
                    future.cancel();
                    emit(Err("cancelled".to_string()));
                    return "cancelled".to_string();
                }
                let count_used = progress.used.iter().filter(|&&x| !x).count();
                let count_unsorted = progress
                    .unsorted
                    .iter()
                    .map(|&x| x.count_ones() as usize)
                    .sum::<usize>();
                if progress.branches >= next_progress
                    || count_used != prev_used
                    || count_unsorted != prev_unsorted
                {
                    let svg = gen_svg(&pos, &progress);
                    emit(Ok(Some(ProgressUpdate {
                        n,
                        l,
                        d: pos.d,
                        max_branches: FIB1[n] as _,
                        branches: progress.branches as _,
                        used: progress.used,
                        unsorted: progress
                            .unsorted
                            .iter()
                            .map(|&x| {
                                (0..crate::sorting_network_check::State::BITS)
                                    .map(|i| (x >> i) & 1 != 0)
                                    .collect()
                            })
                            .collect(),
                        svg,
                        time: begin_time.elapsed().as_millis() as u64,
                    })));
                    let percent = (progress.branches * 100) / fib1_n;
                    next_progress = ((percent + 1) * fib1_n - 1) / 100 + 1;
                    prev_used = count_used;
                    prev_unsorted = count_unsorted;
                }
                if progress.branches >= fib1_n {
                    emit(Ok(None));
                    break;
                }
            }
            */
        }
        Err(e) => {
            emit(EmitType::Error(e));
        }
    }

    "done verify".to_string()
}

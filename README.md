# Sorting Network Verification Visualiser

This application is created with the purpose of visualizing the solver for the following competitive programming problem.

- [yukicoder No.3047 Verification of Sorting Network (for Japanese)](https://yukicoder.me/problems/11776) 
- [Problem and Explanation, Reference Translation to English by GPT](https://gist.github.com/mizar/dbed8f81e1b9f483eaf12dd22a50e3a9)

## Input Format

The input format for the network in this application follows the style of competitive programming problems as shown below.

> $N\ M$<br>
> $A_1\ A_2\ A_3 \dots A_M$<br>
> $B_1\ B_2\ B_3 \dots B_M$<br>

- $N:$ Length of the input and output sequences
- $M:$ Number of comparator exchanges (CEs)
- $A_i, B_i:$ Targets of each comparator exchange

Constraints:

- All inputs are integers.
- $2\leq N\leq 64$
- $1\leq M$
- $1\leq A_i\leq B_i\leq N\quad(1\leq i\leq M)$

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

## Setups

```
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install tauri-cli --version "^2.0.0" --locked
```

- develop

```
cargo tauri dev
```

- release build

```
cargo tauri build
```

- icon update

```
cargo tauri icon <imagefile_path>
```

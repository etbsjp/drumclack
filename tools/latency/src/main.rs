//! `latency` CLI: WAV ファイルから、打鍵音（クリック音）とそれに続く発音（キック音等）の
//! 立ち上がりの時間差（レイテンシ）を計測する。
//!
//! 使い方の詳細は `tools/latency/README.md` を参照。

use clap::Parser;
use latency::{analyze_wav_file, DetectConfig};
use std::path::PathBuf;
use std::process::ExitCode;

/// WAV ファイルから、打鍵音（クリック）→発音（キック等）の遅延（レイテンシ）を計測する。
#[derive(Parser, Debug)]
#[command(name = "latency", version, about, long_about = None)]
struct Cli {
    /// 解析対象の WAV ファイル（PCM 16/24/32bit または 32bit float）。
    input: PathBuf,

    /// 打鍵音（クリック）の立ち上がりとみなす振幅のしきい値（0.0〜1.0、フルスケール比）。
    /// 録音の環境音・ノイズフロアより大きく、打鍵音の音量より小さい値にする。
    #[arg(long, default_value_t = DetectConfig::default().click_threshold)]
    click_threshold: f32,

    /// 発音（キック等）の立ち上がりとみなす振幅のしきい値（0.0〜1.0、フルスケール比）。
    /// 打鍵音の音量より大きく、発音の音量より小さい値にする。`click_threshold` より大きい必要がある。
    #[arg(long, default_value_t = DetectConfig::default().kick_threshold)]
    kick_threshold: f32,

    /// 打鍵音の後、発音を探す最大時間（ミリ秒）。この時間内に発音が見つからない場合は
    /// 「未ペア（発音なし）」として扱う。
    #[arg(long, default_value_t = DetectConfig::default().max_window_ms)]
    max_window_ms: f64,

    /// 発音（または未ペアの打鍵音）を検出した後、次の打鍵音の検出を再開するために必要な、
    /// 振幅が click_threshold を連続して下回っている「静けさ」の継続時間（ミリ秒）。
    /// 発音の減衰・余韻を次の打鍵音と誤検出しないための設定。
    #[arg(long, default_value_t = DetectConfig::default().quiet_ms)]
    quiet_ms: f64,

    /// エンベロープフォロワーの release 時間（ミリ秒）。減衰中の音の0点交差を無視するために
    /// ピーク値を保持しておく時間。通常は変更不要。
    #[arg(long, default_value_t = DetectConfig::default().release_ms)]
    release_ms: f64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.kick_threshold <= cli.click_threshold {
        eprintln!(
            "エラー: --kick-threshold ({}) は --click-threshold ({}) より大きい値にしてください。",
            cli.kick_threshold, cli.click_threshold
        );
        return ExitCode::FAILURE;
    }

    let cfg = DetectConfig {
        click_threshold: cli.click_threshold,
        kick_threshold: cli.kick_threshold,
        max_window_ms: cli.max_window_ms,
        quiet_ms: cli.quiet_ms,
        release_ms: cli.release_ms,
    };

    let result = match analyze_wav_file(&cli.input, &cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("エラー: {e}");
            return ExitCode::FAILURE;
        }
    };

    if result.is_empty() {
        println!("検出0組: 打鍵音（クリック）→発音（キック等）のペアを検出できませんでした。");
        println!(
            "  （無音、しきい値が合っていない、または発音が一度も鳴っていない可能性があります。"
        );
        println!("   --click-threshold / --kick-threshold を調整して再実行してください。）");
        if result.unmatched_clicks > 0 {
            println!(
                "  打鍵音らしき立ち上がりは {} 回検出しましたが、発音が見つかりませんでした。",
                result.unmatched_clicks
            );
        }
        return ExitCode::SUCCESS;
    }

    for (i, pair) in result.pairs.iter().enumerate() {
        println!(
            "組{:>3}: click={:>9.3}ms  kick={:>9.3}ms  遅延={:>8.3}ms",
            i + 1,
            pair.click_ms,
            pair.kick_ms,
            pair.delay_ms
        );
    }

    if result.unmatched_clicks > 0 {
        println!(
            "\n警告: 打鍵音は検出したが {} ms 以内に発音が見つからなかった打鍵音が {} 回ありました（未ペア、集計対象外）。",
            cli.max_window_ms, result.unmatched_clicks
        );
    }

    let median = result.median_delay_ms().unwrap();
    let max = result.max_delay_ms().unwrap();
    println!(
        "\n検出{}組: 中央値={:.3}ms  最大値={:.3}ms",
        result.pairs.len(),
        median,
        max
    );

    ExitCode::SUCCESS
}

# VVVST / VoiceVox VST

Voicevox の VST プラグイン。

エディタ側：<https://github.com/sevenc-nanashi/voicevox/tree/add/vst>  
Issue：<https://github.com/VOICEVOX/voicevox_project/issues/45>

## 開発

- エディタをクローンして`npm run vst:serve`すると VST 用のエディタが立ち上がります
- Release ビルドするときはエディタを`npm run vst:build`し、`dist`内を`resources/editor`にコピーしてください

## ビルド

### VST プラグイン本体

```bash
❯ cargo xtask build -h
Usage: xtask.exe build [OPTIONS]

Options:
  -r, --release                          Releaseビルドを行うかどうか。
  -l, --log                              logs内にVST内のログを出力するかどうか。
  -d, --dev-server-url <DEV_SERVER_URL>  開発用サーバーのURL。デフォルトはhttp://localhost:5173。
  -h, --help                             Print help
  -V, --version                          Print version
```

### Windows用インストーラー

依存：
- [NSIS](https://nsis.sourceforge.io/Main_Page)（3.09 で動作確認）

```bash
❯ cargo xtask installer -h
# TODO
```

## 仕組み

```mermaid
sequenceDiagram
    participant daw as DAW（VST3ホスト）
    participant cpp as VVVST（C++）
    participant rust as VVVST（Rust）
    participant vue as Voicevox Editor

    daw->>cpp: 音声取得（run）
    cpp->>rust: plugin_run
    rust->>cpp: 書き込んで返す
    cpp->>daw: 波形送信
    daw->>cpp: 再生情報
    opt 再生情報が変更されたら
      cpp->>rust: plugin_run
      rust->>vue: 情報送信
      Note over vue: UIロックとか再生位置移動とか
    end

    opt エディタのフレーズが更新されたら
        vue->>rust: タイミング、SingingVoiceKey
        rust->>vue: 不足しているSingingVoiceKey
        vue->>rust: SingingVoice
        Note over rust: wavパース&再サンプル->ミックスダウン作成 @ 別スレッド
    end

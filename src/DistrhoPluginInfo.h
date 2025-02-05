#ifndef DISTRHO_PLUGIN_INFO_H_INCLUDED
#define DISTRHO_PLUGIN_INFO_H_INCLUDED

// AUのビルドが落ちる問題へのワークアラウンド。
// ここで<vector>をincludeすると何故か治る。
#include <vector>

// #define DISTRHO_PLUGIN_BRAND "Nanashi."
#define DISTRHO_PLUGIN_BRAND "Voicevox"
#ifdef DEBUG
#define DISTRHO_PLUGIN_NAME "VOICEVOX (Debug)"
#else
#define DISTRHO_PLUGIN_NAME "VOICEVOX"
#endif
#define DISTRHO_PLUGIN_URI "https://github.com/sevenc-nanashi/vvvst-dpf-rust/"

// #define DISTRHO_PLUGIN_BRAND_ID Vcvx
#define DISTRHO_PLUGIN_BRAND_ID ScNs

#ifdef DEBUG
#define DISTRHO_PLUGIN_UNIQUE_ID VvsD
#else
#define DISTRHO_PLUGIN_UNIQUE_ID Vvst
#endif

// #define DISTRHO_PLUGIN_CLAP_ID "jp.hiroshiba.vvvst"
#ifdef DEBUG
#define DISTRHO_PLUGIN_CLAP_ID "com.sevenc-nanashi.vvvst-dpf-rust-debug"
#else
#define DISTRHO_PLUGIN_CLAP_ID "com.sevenc-nanashi.vvvst-dpf-rust"
#endif

#define DISTRHO_PLUGIN_HAS_UI 1
#define DISTRHO_PLUGIN_IS_SYNTH 1
#define DISTRHO_PLUGIN_IS_RT_SAFE 1
#define DISTRHO_PLUGIN_NUM_INPUTS 0
#define DISTRHO_PLUGIN_NUM_OUTPUTS 64
#define DISTRHO_PLUGIN_WANT_TIMEPOS 1
#define DISTRHO_PLUGIN_WANT_STATE 1
#define DISTRHO_PLUGIN_WANT_FULL_STATE 1
#define DISTRHO_PLUGIN_WANT_DIRECT_ACCESS 1
#define DISTRHO_UI_USE_EXTERNAL 1
#define DISTRHO_UI_USER_RESIZABLE 1
#define DISTRHO_UI_DEFAULT_WIDTH 1080
#define DISTRHO_UI_DEFAULT_HEIGHT 720

#define DISTRHO_PLUGIN_VST3_CATEGORIES "Instrument|Synth"

#endif // DISTRHO_PLUGIN_INFO_H_INCLUDED

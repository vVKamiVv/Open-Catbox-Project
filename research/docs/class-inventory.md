# Class inventory (generated)

Every C++ class with RTTI in the v1.0.3 executable, generated from `re/rtti_inventory.tsv` by
`tools/re/make_class_inventory.py`. Do not edit by hand; regenerate after re-running `RttiInventory.java`.

284 classes. Addresses are for Ghidra's Switch loader (base `0x7100000000`). "Methods" counts the
entries of the primary vtable; `re/symbols.csv` names the ones unique to a class.

## Script commands (`ADV::*`) (66)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::ADV::AUTOSAVE` | `71001d2a90` | `71001d2a68` | 5 |
| `shin::ADV::BGMPLAY` | `71001d2540` | `71001d2518` | 5 |
| `shin::ADV::BGMSTOP` | `71001d2590` | `71001d2568` | 5 |
| `shin::ADV::BGMSYNC` | `71001d2680` | `71001d2658` | 5 |
| `shin::ADV::BGMVOL` | `71001d25e0` | `71001d25b8` | 5 |
| `shin::ADV::BGMWAIT` | `71001d2630` | `71001d2608` | 5 |
| `shin::ADV::CHARS` | `71001d3688` | `71001d3660` | 5 |
| `shin::ADV::Command` | `71001d2008` | `71001d2068`, `71001d20b8`, `71001d2108`, `71001d2158`, `71001d21a8`, `71001d21f8`, `71001d2248`, `71001d2298`, `71001d22e8`, `71001d2338`, `71001d2388`, `71001d23d8`, `71001d24b8`, `71001d2508`, `71001d2558`, `71001d25a8`, `71001d25f8`, `71001d2648`, `71001d2698`, `71001d26e8`, `71001d2738`, `71001d2788`, `71001d27d8`, `71001d2828`, `71001d2878`, `71001d28c8`, `71001d2918`, `71001d2968`, `71001d29b8`, `71001d2a08`, `71001d2a58`, `71001d2aa8`, `71001d2af8`, `71001d2b48`, `71001d2b98`, `71001d2be8`, `71001d2c38`, `71001d2c88`, `71001d2cd8`, `71001d2d28`, `71001d2e20`, `71001d2e70`, `71001d2f58`, `71001d3040`, `71001d3128`, `71001d3178`, `71001d31c8`, `71001d32b0`, `71001d3300`, `71001d33e8`, `71001d3438`, `71001d3488`, `71001d3570`, `71001d3650`, `71001d36a0`, `71001d36f0`, `71001d3740`, `71001d37a8`, `71001d3878`, `71001d38c8` | 0 |
| `shin::ADV::Context` | `71001d3e08` | `71001d38e0` | 3 |
| `shin::ADV::DEBUGOUT` | `71001d38b0` | `71001d3888` | 5 |
| `shin::ADV::EVBEGIN` | `71001d2ae0` | `71001d2ab8` | 5 |
| `shin::ADV::EVEND` | `71001d2b30` | `71001d2b08` | 5 |
| `shin::ADV::EXIT` | `71001d20a0` | `71001d2078` | 5 |
| `shin::ADV::First` | `71001d4668` | `71001d4458` | 5 |
| `shin::ADV::HiddenWindow` | `71001d4698` | `71001d44c8`, `71001d4540` | 13 |
| `shin::ADV::LAYERCTRL` | `71001d2f40` | `71001d2f18` | 5 |
| `shin::ADV::LAYERINIT` | `71001d2d10` | `71001d2ce8` | 5 |
| `shin::ADV::LAYERLOAD` | `71001d2e08` | `71001d2de0` | 5 |
| `shin::ADV::LAYERSELECT` | `71001d3160` | `71001d3138` | 5 |
| `shin::ADV::LAYERSWAP` | `71001d3110` | `71001d30e8` | 5 |
| `shin::ADV::LAYERUNLOAD` | `71001d2e58` | `71001d2e30` | 5 |
| `shin::ADV::LAYERWAIT` | `71001d3028` | `71001d3000` | 5 |
| `shin::ADV::MASKLOAD` | `71001d3558` | `71001d3530` | 5 |
| `shin::ADV::MASKUNLOAD` | `71001d3638` | `71001d3610` | 5 |
| `shin::ADV::MOVIEWAIT` | `71001d31b0` | `71001d3188` | 5 |
| `shin::ADV::MSGCLOSE` | `71001d2370` | `71001d2348` | 5 |
| `shin::ADV::MSGINIT` | `71001d21e0` | `71001d21b8` | 5 |
| `shin::ADV::MSGSET` | `71001d2230` | `71001d2208` | 5 |
| `shin::ADV::MSGSIGNAL` | `71001d22d0` | `71001d22a8` | 5 |
| `shin::ADV::MSGSYNC` | `71001d2320` | `71001d22f8` | 5 |
| `shin::ADV::MSGWAIT` | `71001d2280` | `71001d2258` | 5 |
| `shin::ADV::NOP` | `71001d2050` | `71001d2028` | 5 |
| `shin::ADV::NOTIFYSET` | `71001d3860` | `71001d3838` | 5 |
| `shin::ADV::PAGEBACK` | `71001d33d0` | `71001d33a8` | 5 |
| `shin::ADV::PLANECLEAR` | `71001d3470` | `71001d3448` | 5 |
| `shin::ADV::PLANESELECT` | `71001d3420` | `71001d33f8` | 5 |
| `shin::ADV::QUIZ` | `71001d3728` | `71001d3700` | 5 |
| `shin::ADV::RESUME` | `71001d2bd0` | `71001d2ba8` | 5 |
| `shin::ADV::RESUMESET` | `71001d2b80` | `71001d2b58` | 5 |
| `shin::ADV::SAVEINFO` | `71001d2a40` | `71001d2a18` | 5 |
| `shin::ADV::SELECT` | `71001d23c0` | `71001d2398` | 5 |
| `shin::ADV::SEONCE` | `71001d28b0` | `71001d2888` | 5 |
| `shin::ADV::SEPAN` | `71001d2810` | `71001d27e8` | 5 |
| `shin::ADV::SEPLAY` | `71001d26d0` | `71001d26a8` | 5 |
| `shin::ADV::SESTOP` | `71001d2720` | `71001d26f8` | 5 |
| `shin::ADV::SESTOPALL` | `71001d2770` | `71001d2748` | 5 |
| `shin::ADV::SEVOL` | `71001d27c0` | `71001d2798` | 5 |
| `shin::ADV::SEWAIT` | `71001d2860` | `71001d2838` | 5 |
| `shin::ADV::SGET` | `71001d20f0` | `71001d20c8` | 5 |
| `shin::ADV::SHOWCHARS` | `71001d3790` | `71001d3768` | 5 |
| `shin::ADV::SSET` | `71001d2140` | `71001d2118` | 5 |
| `shin::ADV::SYSCALL` | `71001d2c20` | `71001d2bf8` | 5 |
| `shin::ADV::SYSSE` | `71001d29f0` | `71001d29c8` | 5 |
| `shin::ADV::SceneLoader` | `71001d4680` | `71001d4490` | 5 |
| `shin::ADV::SystemMenu` | `71001d46d0` | `71001d45a0`, `71001d4618` | 13 |
| `shin::ADV::TIPSGET` | `71001d36d8` | `71001d36b0` | 5 |
| `shin::ADV::TRANSSET` | `71001d3298` | `71001d3270` | 5 |
| `shin::ADV::TRANSWAIT` | `71001d32e8` | `71001d32c0` | 5 |
| `shin::ADV::TROPHY` | `71001d2c70` | `71001d2c48` | 5 |
| `shin::ADV::UNLOCK` | `71001d2cc0` | `71001d2c98` | 5 |
| `shin::ADV::VOICEPLAY` | `71001d2900` | `71001d28d8` | 5 |
| `shin::ADV::VOICESTOP` | `71001d2950` | `71001d2928` | 5 |
| `shin::ADV::VOICEWAIT` | `71001d29a0` | `71001d2978` | 5 |
| `shin::ADV::WAIT` | `71001d2190` | `71001d2168` | 5 |
| `shin::ADV::WIPE` | `71001d24a0` | `71001d2478` | 5 |
| `shin::ADV::WIPEWAIT` | `71001d24f0` | `71001d24c8` | 5 |

## Layers and layer loaders (53)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::AnimationLayer` | `71001d5728` | `71001d4df8`, `71001d4ea0`, `71001d5830`, `71001dd9d0`, `71001de090`, `71002267b8` | 5 |
| `shin::AnimationLayerLoader` | `71001da428` | `71001da2b0` | 9 |
| `shin::BustupLayer` | `71001d62e0` | `71001d6220`, `71001d62c0`, `71002265a8` | 18 |
| `shin::BustupLayer::BlockCache::Checker` | `71001d6328` | `71001d6308` | 4 |
| `shin::BustupLayer::Listener` | `71001d3e40` | `71001d3eb0` | 0 |
| `shin::BustupLayerLoader` | `71001da3b0` | `71001da0f8` | 9 |
| `shin::CakeLayer` | `71001d6d98` | `71001d6cd8`, `71001d6d78` | 18 |
| `shin::CakeLayer::CakeOverlayLayer` | `71001d6ec0` | `71001d6dc0`, `71001d6ea0` | 26 |
| `shin::DrawableLayer` | `71001d9cc8` | `71001d3758`, `71001d9ae0`, `71001d9b68`, `71001d9cf8`, `71001ddb58`, `7100226580` | 5 |
| `shin::EffectLayer` | `71001d8f10` | `71001d8e20`, `71001d8ec8`, `71001d8ef8` | 19 |
| `shin::EffectLayerLoader` | `71001da3e0` | `71001da1a8` | 9 |
| `shin::FocusLineLayer` | `71001d9448` | `71001d9388`, `71001d9428` | 18 |
| `shin::FocusLineLayerLoader` | `71001da3f8` | `71001da200` | 9 |
| `shin::InteractiveLayer` | `71001d3740` | `71001d6db0`, `71001d9730`, `71001dbc88`, `71001dde58`, `71002266c0` | 0 |
| `shin::LastLayer` | `71001d9718` | `71001d9658`, `71001d96f8` | 18 |
| `shin::LastLayer::LastOverlayLayer` | `71001d9840` | `71001d9740`, `71001d9820` | 26 |
| `shin::Layer` | `71001d9c58` | `71001d4328`, `71001d99e8`, `71001d9a50`, `71001d9ce0`, `71001d9fc8`, `71001dcf40`, `7100221d10`, `7100226548` | 5 |
| `shin::Layer::Effectable` | `71001d5920` | `71001d8f40`, `71001d9fd8`, `7100226820` | 0 |
| `shin::Layer::Properties` | `71001d9c90` | `71001d9a80`, `71001d9ab0` | 4 |
| `shin::LayerGroup` | `71001d9ec8` | `71001d9e08`, `71001d9ea8`, `7100226550` | 18 |
| `shin::LayerGroup::TransitionLayer` | `71001d9fa8` | `71001d9ef0`, `71001d9f60`, `71001d9f90`, `7100226da0` | 12 |
| `shin::LayerLoader` | `71001da350` | `71001da380`, `71001da398`, `71001da3b0`, `71001da3c8`, `71001da3e0`, `71001da3f8`, `71001da410`, `71001da428`, `71001da440`, `71001da458` | 0 |
| `shin::MovieLayer` | `71001dd9b8` | `71001dd8b8`, `71001dd960`, `7100226638` | 19 |
| `shin::MovieLayer::Shared` | `71001dd980` | `71001dd850`, `71001dd888` | 5 |
| `shin::MovieLayerLoader` | `71001da3c8` | `71001da150` | 9 |
| `shin::MurdererLayer` | `71001dbc70` | `71001dbb10`, `71001dbbb0` | 18 |
| `shin::MurdererLayer::Base` | `71001dbbd0` | `71001dbbf8`, `71001dbc10`, `71001dbc28`, `71001dbc40`, `71001dbc58`, `71001dbc70` | 0 |
| `shin::MurdererLayer::Check` | `71001dbc40` | `71001db8e0` | 18 |
| `shin::MurdererLayer::HintMenu` | `71001dbc28` | `71001db840` | 18 |
| `shin::MurdererLayer::MurdererOverlayLayer` | `71001dbd98` | `71001dbc98`, `71001dbd78` | 26 |
| `shin::MurdererLayer::ReplayMenu` | `71001dbbf8` | `71001db6a0` | 18 |
| `shin::MurdererLayer::Rule` | `71001dbc10` | `71001db7a0` | 18 |
| `shin::MurdererLayer::Select` | `71001dbc58` | `71001dba10` | 18 |
| `shin::MurdererLayer::TopMenu` | `71001dbbe0` | `71001db5c0` | 18 |
| `shin::NewDrawableLayer` | `71001d9ce0` | `71001d5740`, `71001d62f8`, `71001d8f30`, `71001d9460`, `71001d9b98`, `71001d9c38`, `71001d9ee0`, `71001dbe98`, `71001ddc48`, `7100221c50`, `71002239c0` | 5 |
| `shin::NullLayer` | `71001ddb40` | `71001dda98`, `71001ddb20` | 15 |
| `shin::NullLayerLoader` | `71001da368` | `71001d9ff0` | 9 |
| `shin::OverlayLayer` | `71001d4310` | `71001d6ed8`, `71001d9858`, `71001dbdb0`, `71001ddf80`, `7100222f70`, `7100226730` | 0 |
| `shin::PageLayer` | `71001dbe80` | `71001dbdc0`, `71001dbe60`, `7100227590` | 18 |
| `shin::PictureLayer` | `71001ddc30` | `71001ddb70`, `71001ddc10` | 18 |
| `shin::PictureLayer::BlockCache::Checker` | `71001ddc78` | `71001ddc58` | 4 |
| `shin::PictureLayerLoader` | `71001da398` | `71001da0a0` | 9 |
| `shin::QuizLayer` | `71001dde40` | `71001ddd80`, `71001dde20` | 18 |
| `shin::QuizLayer::QuizOverlayLayer` | `71001ddf68` | `71001dde68`, `71001ddf48` | 26 |
| `shin::QuizLayerLoader` | `71001da440` | `71001da308` | 9 |
| `shin::RainLayer` | `71001de078` | `71001ddf90`, `71001de038` | 19 |
| `shin::RainLayerLoader` | `71001da410` | `71001da258` | 9 |
| `shin::SakuraAnimationLayer` | `71001d5818` | `71001d5750`, `71001d57f8` | 19 |
| `shin::ScreenLayer` | `7100221c38` | `7100221b78`, `7100221c18`, `7100226760` | 18 |
| `shin::ScreenLayer::TransitionLayer` | `7100221cf8` | `7100221c60`, `7100221cd8`, `7100227598` | 13 |
| `shin::SelectLayer` | `7100222f58` | `7100222e58`, `7100222f38` | 26 |
| `shin::TileLayer` | `71002239a8` | `71002238e8`, `7100223988` | 18 |
| `shin::TileLayerLoader` | `71001da380` | `71001da048` | 9 |

## Transitions (wipers) (23)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::DefaultWiper` | `71001d8cf8` | `71001d8c80`, `71001d8cd8`, `7100226c70` | 9 |
| `shin::DefaultWiperLoader` | `7100225758` | `71002253e0` | 9 |
| `shin::GlassWiper` | `71001d9610` | `71001d9598`, `71001d95f0`, `7100226d10` | 9 |
| `shin::GlassWiperLoader` | `7100225830` | `71002256f8` | 9 |
| `shin::MaskWiper` | `71001dbfe8` | `71001dbf70`, `71001dbfc8`, `7100226fe8` | 9 |
| `shin::MaskWiperLoader` | `7100225770` | `7100225438` | 9 |
| `shin::RippleWiper` | `7100220800` | `7100220788`, `71002207e0`, `7100227478` | 9 |
| `shin::RippleWiperLoader` | `7100225800` | `7100225648` | 9 |
| `shin::ScanlineWiper` | `7100221580` | `7100221508`, `7100221560`, `7100227538` | 9 |
| `shin::ScanlineWiperLoader` | `71002257e8` | `71002255f0` | 9 |
| `shin::ScrollWiper` | `7100222e30` | `7100222db8`, `7100222e10`, `71002275c8` | 9 |
| `shin::ScrollWiperLoader` | `7100225788` | `7100225490` | 9 |
| `shin::TimedWiper` | `71002253b8` | `71001d8d10`, `71001d9628`, `71001dc000`, `7100220818`, `7100221598`, `7100222e48`, `7100225060`, `7100225290`, `7100225330`, `7100225340`, `7100225398`, `7100225998`, `7100228590` | 4 |
| `shin::TurnWiper` | `7100225048` | `7100224fd0`, `7100225028`, `7100228550` | 9 |
| `shin::TurnWiperLoader` | `71002257a0` | `71002254e8` | 9 |
| `shin::WaveWiper` | `7100225278` | `7100225200`, `7100225258`, `7100228570` | 9 |
| `shin::WaveWiperLoader` | `71002257d0` | `7100225598` | 9 |
| `shin::WhirlWiper` | `7100225318` | `71002252a0`, `71002252f8`, `7100228580` | 9 |
| `shin::WhirlWiperLoader` | `7100225818` | `71002256a0` | 9 |
| `shin::Wiper` | `71001d8d10` | `71002253d0`, `7100226c68` | 0 |
| `shin::WiperLoader` | `7100225740` | `7100225770`, `7100225788`, `71002257a0`, `71002257b8`, `71002257d0`, `71002257e8`, `7100225800`, `7100225818`, `7100225830`, `7100225848` | 0 |
| `shin::ZoomWiper` | `7100225980` | `7100225908`, `7100225960`, `7100228610` | 9 |
| `shin::ZoomWiperLoader` | `71002257b8` | `7100225540` | 9 |

## Message window and text (29)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::MessageBrowser` | `71001dc310` | `71001dc010`, `71001dc088` | 13 |
| `shin::MessageBrowser::ChapterSelector` | `71001dc360` | `71001dc248`, `71001dc2c0` | 13 |
| `shin::MessageBrowser::First` | `71001dc348` | `71001dc210` | 5 |
| `shin::MessageBrowser::Layouter` | `71001dc6a8` | `71001dc5b8` | 30 |
| `shin::MessageBrowser::LogBuilder` | `71001dc590` | `71001dc3a8` | 61 |
| `shin::MessageLayer` | `71001dcf28` | `71001dcd58`, `71001dcdc0`, `7100226710` | 11 |
| `shin::MessageLayer::Block` | `71001dce70` | `71001dce98`, `71001dceb0`, `71001dcec8`, `71001dcee0`, `71001dcef8`, `71001dcf10`, `71002270d8` | 0 |
| `shin::MessageLayer::Layouter` | `71001dcf10` | `71001dcc58` | 30 |
| `shin::MessageLayer::Listener` | `71001d3e50` | `71001d3ec0` | 0 |
| `shin::MessageLayer::Section` | `71001dceb0` | `71001dcf90`, `71002270f0` | 2 |
| `shin::MessageLayer::Sync` | `71001dcec8` | `71001dcfb0`, `71002270f8` | 2 |
| `shin::MessageLayer::Voice` | `71001dce80` | `71001dcf50`, `71002270e0` | 2 |
| `shin::MessageLayer::VoiceSync` | `71001dcee0` | `71001dcfd0`, `7100227100` | 2 |
| `shin::MessageLayer::VoiceWait` | `71001dcef8` | `71001dcff0`, `7100227108` | 2 |
| `shin::MessageLayer::Wait` | `71001dce98` | `71001dcf70`, `71002270e8` | 2 |
| `shin::MessageTextLayouter` | `71001dd190` | `71001dc6c0`, `71001dcf28`, `71001dd090` | 30 |
| `shin::MessageTextLayouter::Char` | `71001dc6d0` | `71001dd1d8`, `7100227020` | 2 |
| `shin::MessageTextLayouter::Command` | `71001dc6c0` | `71001dc6e8`, `71001dcdf8`, `71001dce10`, `71001dce28`, `71001dce40`, `71001dce58`, `71001dce70`, `7100227018` | 0 |
| `shin::MessageTextLayouter::Section` | `71001dce10` | `71001dd1f8`, `71002270a8` | 2 |
| `shin::MessageTextLayouter::Sync` | `71001dce28` | `71001dd218`, `71002270b0` | 2 |
| `shin::MessageTextLayouter::Voice` | `71001dcde0` | `71001dd238`, `71002270c8` | 2 |
| `shin::MessageTextLayouter::VoiceSync` | `71001dce40` | `71001dd258`, `71002270b8` | 2 |
| `shin::MessageTextLayouter::VoiceWait` | `71001dce58` | `71001dd278`, `71002270c0` | 2 |
| `shin::MessageTextLayouter::Wait` | `71001dcdf8` | `71001dd1b8`, `71002270d0` | 2 |
| `shin::MessageTextParser` | `71001dd2c0` | `71001dd298` | 3 |
| `shin::MessageVoicePlayer::Layouter` | `71001dd3d0` | `71001dd2e8` | 29 |
| `shin::PlainTextParser` | `71001ddd38` | `71001ddd20` | 3 |
| `shin::TextLayouter` | `71001dd180` | `71001dd1a8`, `71001dd3e8` | 0 |
| `shin::TextParser` | `71001dd2b0` | `71001dd2d8`, `71001ddd50` | 0 |

## Menus and modes (60)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::BGMMode` | `71001d5ee8` | `71001d5df0`, `71001d5e60` | 12 |
| `shin::BGMMode::First` | `71001d5f20` | `71001d5ec0` | 5 |
| `shin::BustupMode` | `71001d65a0` | `71001d63d0`, `71001d6440` | 12 |
| `shin::BustupMode::First` | `71001d65d8` | `71001d64a0` | 5 |
| `shin::BustupMode::Viewer` | `71001d65f0` | `71001d64d8`, `71001d6550` | 13 |
| `shin::CGMode` | `71001d70b8` | `71001d6ee8`, `71001d6f58` | 12 |
| `shin::CGMode::First` | `71001d70f0` | `71001d6fb8` | 5 |
| `shin::CGMode::Viewer` | `71001d7108` | `71001d6ff0`, `71001d7068` | 13 |
| `shin::CharsMode` | `71001d77b0` | `71001d76b0`, `71001d7728` | 13 |
| `shin::CharsMode` | `710005580c` | — | 0 |
| `shin::CharsMode` | `710005593c` | — | 0 |
| `shin::CharsMode` | `7100055a7c` | — | 0 |
| `shin::CharsMode` | `7100055bac` | — | 0 |
| `shin::CharsMode` | `7100055ddc` | — | 0 |
| `shin::CharsMode` | `710005600c` | — | 0 |
| `shin::CharsMode` | `710005614c` | — | 0 |
| `shin::CharsMode` | `71000563dc` | — | 0 |
| `shin::CharsMode` | `7100057a4c` | — | 0 |
| `shin::CharsMode` | `710005f3fc` | — | 0 |
| `shin::CharsMode` | `710005f4bc` | — | 0 |
| `shin::CharsMode` | `710005f57c` | — | 0 |
| `shin::CharsMode` | `710005f63c` | — | 0 |
| `shin::CharsMode` | `710005f6fc` | — | 0 |
| `shin::CharsMode` | `710005f7cc` | — | 0 |
| `shin::CharsMode` | `710005f88c` | — | 0 |
| `shin::CharsMode` | `710005f94c` | — | 0 |
| `shin::CharsMode` | `710005fa0c` | — | 0 |
| `shin::CharsMode` | `710005facc` | — | 0 |
| `shin::CharsMode` | `710005fb8c` | — | 0 |
| `shin::CharsMode` | `710005fc4c` | — | 0 |
| `shin::CharsMode` | `710005fd0c` | — | 0 |
| `shin::CharsMode` | `710006002c` | — | 0 |
| `shin::CharsMode` | `71000600dc` | — | 0 |
| `shin::CharsMode` | `71000603fc` | — | 0 |
| `shin::CharsMode` | `710006052c` | — | 0 |
| `shin::CharsMode::First` | `71001d77e8` | `71001d7788` | 5 |
| `shin::ConfigMenu` | `71001d7f00` | `71001d7dc0`, `71001d7e38` | 13 |
| `shin::ConfigMenu::ButtonMenuItem` | `71001d7ed0` | `71001d7cc0` | 14 |
| `shin::ConfigMenu::First` | `71001d7f38` | `71001d7e98` | 5 |
| `shin::ConfigMenu::MenuItem` | `71001d7ec0` | `71001d7ee8`, `71001d7f00` | 0 |
| `shin::ConfigMenu::SliderMenuItem` | `71001d7ee8` | `71001d7d40` | 14 |
| `shin::FirstBoot` | `71001d90d0` | `71001d90b0` | 4 |
| `shin::LoadGame` | `71002208a0` | `7100220860` | 5 |
| `shin::ManualViewer` | `71001da778` | `71001da678`, `71001da6f0` | 13 |
| `shin::ManualViewer::First` | `71001da7b0` | `71001da750` | 5 |
| `shin::MovieMode` | `71001dac58` | `71001daa88`, `71001daaf8` | 12 |
| `shin::MovieMode::First` | `71001dac90` | `71001dab58` | 5 |
| `shin::MovieMode::Viewer` | `71001daca8` | `71001dab90`, `71001dac08` | 13 |
| `shin::SaveGame` | `7100220888` | `7100220828` | 5 |
| `shin::SaveMenu` | `7100220d48` | `7100220c28`, `7100220ca0` | 13 |
| `shin::SaveMenu::First` | `7100220d80` | `7100220d20` | 5 |
| `shin::ScenarioSelect` | `71002216a0` | `71002215a8`, `7100221618` | 12 |
| `shin::ScenarioSelect::First` | `71002216d8` | `7100221678` | 5 |
| `shin::ScenarioSelectBridge` | `7100221ad0` | `7100221ab0` | 4 |
| `shin::TipsMode` | `7100223ad0` | `71002239d0`, `7100223a48` | 13 |
| `shin::TipsMode::First` | `7100223b08` | `7100223aa8` | 5 |
| `shin::Title::First` | `7100224078` | `7100223fe0` | 5 |
| `shin::Title::SubMenu` | `7100224040` | `7100223f08`, `7100223f80` | 13 |
| `shin::TitleDemo` | `7100224b78` | `7100224a80`, `7100224af0` | 12 |
| `shin::TitleDemo::First` | `7100224bb0` | `7100224b50` | 5 |

## Audio and video (13)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::Audio::Delegate` | `71001d5c60` | `71001d5b90` | 4 |
| `shin::Audio::Voice` | `71001d5c28` | `71001d5c50` | 0 |
| `shin::Audio::Voice::Delegate` | `71001dd5d0` | `71001dd610`, `7100223028`, `7100223350` | 0 |
| `shin::Audio::VoiceImpl` | `71001d5c38` | `71001d5bc8` | 12 |
| `shin::AudioDecoder` | `71001d5db0` | `71001d5de0` | 0 |
| `shin::AudioDecoderImpl` | `71001d5dc8` | `71001d5d78` | 7 |
| `shin::BGMPlayer` | `71001d61f8` | `71001d61d8` | 4 |
| `shin::Movie` | `71001dd5b8` | `71001dd600` | 0 |
| `shin::MovieImpl` | `71001dd5e0` | `71001dd4c0`, `71001dd570`, `71001dd598` | 20 |
| `shin::MovieImpl::StreamReader` | `71001dd678` | `71001dd638` | 6 |
| `shin::StreamSound` | `7100223008` | `7100222f80`, `7100222fb8`, `7100222fe8` | 5 |
| `shin::SystemSound` | `7100223350` | `7100223318` | 4 |
| `shin::VoicePlayer` | `71002251d8` | `71002251b8` | 4 |

## Data, files and assets (20)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `shin::CachedAssets::CacheDataSource` | `71001d6c60` | `71001d6c90`, `71001d6ca8` | 0 |
| `shin::CachedAssets::CacheDataSourceImpl` | `71001d6c78` | `71001d6b98` | 10 |
| `shin::CachedAssets::RangedCacheDataSourceImpl` | `71001d6c90` | `71001d6bf8` | 10 |
| `shin::Data` | `71001d8b68` | `71001d8b98`, `71001dda70`, `71001de110` | 0 |
| `shin::DataImpl` | `71001d8b80` | `71001d8b10` | 11 |
| `shin::DataSource` | `71001d6c48` | `71001d6c78`, `71001d9028` | 0 |
| `shin::DepthStencil` | `71001d8d78` | `71001d8d58` | 4 |
| `shin::File::Handle` | `71001d8f88` | `71001d8fb8` | 0 |
| `shin::File::HandleImpl` | `71001d8fa0` | `71001d8f58` | 6 |
| `shin::FileDataSourceImpl` | `71001d9010` | `71001d8fc8` | 9 |
| `shin::Font` | `71001d9490` | `71001d9470` | 4 |
| `shin::MaskTexture` | `71001dbec8` | `71001dbea8` | 4 |
| `shin::MountableSourceMedia` | `71001dd448` | `71001dd478` | 0 |
| `shin::MountableSourceMediaImpl` | `71001dd460` | `71001dd3f8` | 10 |
| `shin::MutableData` | `71001dda58` | `71001dda88` | 0 |
| `shin::MutableDataImpl` | `71001dda70` | `71001dd9e0` | 15 |
| `shin::RangedDataImpl` | `71001de0f8` | `71001de0a0` | 11 |
| `shin::RenderTexture` | `71002206e0` | `71002206c0` | 4 |
| `shin::SourceMedia` | `71001d5a10` | `71001d5a40`, `71001dd460` | 0 |
| `shin::TXA4` | `7100225090` | `7100225070` | 4 |

## Everything else (20)

| Class | typeinfo | vtable(s) | Methods |
|---|---|---|---|
| `movie::ClientStreamReader` | `71001dd668` | `71001dd690` | 0 |
| `shin::ADV` | `71001d3e70` | `71001d3908`, `71001d3b68`, `71001d3d60`, `71001d3d88`, `71001d3db8` | 64 |
| `shin::AsyncQueue` | `71001d5ab8` | `71001d5a80` | 4 |
| `shin::AsyncQueue::Task` | `71001d5aa0` | `71001d5a50` | 4 |
| `shin::FrameUpdater` | `71001d9578` | `71001d8d30`, `71001d9558`, `71001d9c78`, `71001d9cb0`, `71001dd9b0` | 4 |
| `shin::GestureRecognizer::Listener` | `71001d3e60` | `71001d3ed0`, `71001d46c8`, `71001d4700`, `71001d4de0`, `71001d5f18`, `71001d65d0`, `71001d6620`, `71001d70e8`, `71001d7138`, `71001d77e0`, `71001d7f30`, `71001da7a8`, `71001dac88`, `71001dacd8`, `71001dc340`, `71001dc390`, `7100220d78`, `71002216d0`, `7100223b00`, `7100224038`, `7100224070`, `7100224ba8`, `7100224ec0` | 0 |
| `shin::NonCopyable` | `71001d3f10` | `71001d3f40`, `71001de160`, `7100222da0` | 0 |
| `shin::RefCounted` | `71001de168` | `71001d5a28`, `71001d5ab8`, `71001d5ad0`, `71001d5dc8`, `71001d6210`, `71001d6340`, `71001d6c60`, `71001d8b80`, `71001d8be0`, `71001d8d40`, `71001d8d90`, `71001d8fa0`, `71001d94a8`, `71001d9c88`, `71001d9cc0`, `71001da368`, `71001dbee0`, `71001dd5d0`, `71001dd9a0`, `71001ddc90`, `71001de120`, `71002206f8`, `7100221d58`, `7100223048`, `7100223368`, `71002250a8`, `71002251f0`, `7100225758` | 4 |
| `shin::RefObject` | `71001de140` | `71001de180` | 0 |
| `shin::RegionAllocator::OutOfMemory` | `71001de1a8` | `71001de190`, `71002272a0` | 0 |
| `shin::Scene` | `71001d3e20` | `71001d3e90`, `71001d5f08`, `71001d65c0`, `71001d70d8`, `71001d90e8`, `71001dac78`, `71002216c0`, `7100221ae8`, `7100224028`, `7100224b98` | 0 |
| `shin::Scripter::Listener` | `71001d3e30` | `71001d3ea0`, `71001dc5a8`, `7100223648` | 0 |
| `shin::SystemEventDelegate` | `71001d5c50` | `71001d5c78`, `71001dd620`, `7100223038` | 0 |
| `shin::Task` | `71001d3f20` | `71001d3f60`, `71001d4680`, `71001d4698`, `71001d46b8`, `71001d46f0`, `71001d4dd0`, `71001d5f38`, `71001d5f88`, `71001d65f0`, `71001d6610`, `71001d7108`, `71001d7128`, `71001d77d0`, `71001d7800`, `71001d7f20`, `71001d7f50`, `71001da798`, `71001da7c8`, `71001daca8`, `71001dacc8`, `71001dc330`, `71001dc360`, `71001dc380`, `71002208a0`, `71002208b8`, `7100220d68`, `7100220d98`, `71002216f0`, `71002233b8`, `7100223af0`, `7100223b20`, `7100224060`, `7100224090`, `7100224bc8`, `7100224f10` | 0 |
| `shin::TaskPool` | `71002233a0` | `7100223378` | 5 |
| `shin::TouchEffect` | `7100224ea8` | `7100224e58` | 10 |
| `shin::Trophy::Installer` | `7100224ef8` | `7100224ed0` | 5 |
| `shin::UITask` | `71001d5f70` | `71001d5f48` | 5 |
| `shin::UpdateTask` | `71001d3f48` | `71001d3ee8` | 5 |
| `std::__1::bad_function_call` | `71001d3f60` | `71001d3f88`, `7100226778` | 0 |

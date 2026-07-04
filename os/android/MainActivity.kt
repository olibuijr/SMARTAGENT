package dev.dioxus.main

import android.webkit.WebView
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewFeature

// dx copies this file verbatim (it does NOT inject the typealias the default
// generated MainActivity carries), so we must reproduce it: sibling generated
// files — e.g. Logger.kt's `BuildConfig.DEBUG` — resolve `dev.dioxus.main`'s
// BuildConfig through this alias. The target is the app's real BuildConfig,
// whose package is the applicationId (derived from the crate name; no
// [bundle].identifier override is set, so it is com.example.SmartagentOs). If a
// bundle identifier is ever added to Dioxus.toml, update this line to match.
typealias BuildConfig = com.example.SmartagentOs.BuildConfig

// Custom MainActivity wired via Dioxus.toml [application].android_main_activity.
//
// Goal: make the system WebView follow the OS dark/light setting so CSS
// `prefers-color-scheme` reports correctly. The app already uses a DayNight
// theme (Dioxus.toml), so `android:isLightTheme` tracks the OS; enabling
// algorithmic darkening lets the WebView map that onto `prefers-color-scheme`.
//
// wry calls WryActivity.onWebViewCreate(webView) right after the WebView is
// built, which we override here. androidx.webkit ships with the generated
// project (implementation "androidx.webkit:webkit:1.13.0"), and the feature is
// guarded so it degrades safely on older WebView providers / pre-API-33.
class MainActivity : WryActivity() {
    override fun onWebViewCreate(webView: WebView) {
        if (WebViewFeature.isFeatureSupported(WebViewFeature.ALGORITHMIC_DARKENING)) {
            WebSettingsCompat.setAlgorithmicDarkeningAllowed(webView.settings, true)
        }
    }
}

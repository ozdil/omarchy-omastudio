#pragma once

#include <QtQml/QQmlApplicationEngine>

/// Registers the `image://omastudio/<kind>/<slot>/<gen>` provider backed by the
/// zero-disk `memfd` preview ring shared with `omastudio-engine daemon`.
void register_omastudio_image_provider(QQmlApplicationEngine &engine);

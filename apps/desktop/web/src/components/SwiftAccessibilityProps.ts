import type { SwiftModifierPresentation } from "./CustomSidebarSurface";

export function swiftAccessibilityProps(
  presentation: SwiftModifierPresentation,
): {
  "aria-hidden"?: boolean;
  "aria-keyshortcuts"?: string;
  "aria-label"?: string;
  "aria-valuetext"?: string;
  "data-swift-accessibility-element-children"?: string;
  "data-swift-accessibility-action"?: string;
  "data-swift-accessibility-action-enabled"?: string;
  "data-swift-accessibility-activation-point"?: string;
  "data-swift-accessibility-activation-point-x"?: string;
  "data-swift-accessibility-activation-point-y"?: string;
  "data-swift-accessibility-hint"?: string;
  "data-swift-accessibility-sort-priority"?: string;
  "data-swift-accessibility-traits"?: string;
  "data-swift-animation"?: string;
  "data-swift-animation-value"?: string;
  "data-swift-alignment-guide"?: string;
  "data-swift-alignment-guide-offset"?: string;
  "data-swift-visual-effect"?: string;
  "data-swift-content-shape"?: string;
  "data-swift-coordinate-space"?: string;
  "data-swift-container-relative-frame-axis"?: string;
  "data-swift-container-relative-frame-count"?: string;
  "data-swift-container-relative-frame-span"?: string;
  "data-swift-container-relative-frame-spacing"?: string;
  "data-swift-container-relative-frame-alignment"?: string;
  "data-swift-content-transition"?: string;
  "data-swift-dynamic-type-size"?: string;
  "data-swift-environment-color-scheme"?: string;
  "data-swift-environment-layout-direction"?: string;
  "data-swift-safe-area-padding-edges"?: string;
  "data-swift-safe-area-padding-length"?: string;
  "data-swift-safe-area-padding-insets"?: string;
  "data-swift-content-margins-edges"?: string;
  "data-swift-content-margins-length"?: string;
  "data-swift-content-margins-placement"?: string;
  "data-swift-content-margins-insets"?: string;
  "data-swift-grid-cell-anchor"?: string;
  "data-swift-grid-cell-columns"?: string;
  "data-swift-grid-column-alignment"?: string;
  "data-swift-scroll-target-behavior"?: string;
  "data-swift-scroll-target-layout"?: string;
  "data-swift-scroll-bounce-behavior"?: string;
  "data-swift-scroll-bounce-axes"?: string;
  "data-swift-scroll-disabled"?: string;
  "data-swift-scroll-position-id"?: string;
  "data-swift-scroll-position-anchor"?: string;
  "data-swift-scroll-position-binding"?: string;
  "data-swift-default-scroll-anchor"?: string;
  "data-swift-tab-view-style"?: string;
  "data-swift-shape-stroke"?: string;
  "data-swift-shape-stroke-color"?: string;
  "data-swift-shape-stroke-width"?: string;
  "data-swift-labels-hidden"?: string;
  "data-swift-control-group-style"?: string;
  "data-swift-draggable"?: string;
  "data-swift-focusable"?: string;
  "data-swift-focused"?: string;
  "data-swift-focused-binding"?: string;
  "data-swift-id"?: string;
  "data-swift-preferred-color-scheme"?: string;
  "data-swift-drop-destination"?: string;
  "data-swift-symbol-effect"?: string;
  "data-swift-symbol-effect-active"?: boolean;
  "data-swift-symbol-effect-value"?: string;
  "data-swift-symbol-effects-removed"?: string;
  "data-swift-on-appear"?: string;
  "data-swift-on-disappear"?: string;
  "data-swift-on-hover"?: string;
  "data-swift-on-geometry-change"?: string;
  "data-swift-on-geometry-change-type"?: string;
  "data-swift-task"?: string;
  "data-swift-task-id"?: string;
  "data-swift-task-id-expression"?: string;
  "data-swift-transition"?: string;
  "data-swift-flips-for-rtl"?: string;
  "data-swift-clip-style"?: string;
  "data-swift-clip-antialiased"?: string;
  "data-swift-allows-hit-testing"?: string;
  "data-swift-hidden"?: string;
  "data-swift-badge"?: string;
  "data-swift-hover-effect"?: string;
  "data-swift-hover-effect-enabled"?: string;
  "data-swift-default-hover-effect"?: string;
  "data-swift-redacted"?: string;
  "data-swift-redaction-reason"?: string;
  "data-swift-privacy-sensitive"?: string;
  "data-swift-unredacted"?: string;
  draggable?: boolean;
} {
  return {
    ...(presentation.ariaHidden !== undefined ? { "aria-hidden": presentation.ariaHidden } : {}),
    ...(presentation.keyboardShortcut !== undefined
      ? { "aria-keyshortcuts": presentation.keyboardShortcut }
      : {}),
    ...(presentation.ariaLabel !== undefined ? { "aria-label": presentation.ariaLabel } : {}),
    ...(presentation.ariaValueText !== undefined
      ? { "aria-valuetext": presentation.ariaValueText }
      : {}),
    ...(presentation.accessibilityHint !== undefined
      ? { "data-swift-accessibility-hint": presentation.accessibilityHint }
      : {}),
    ...(presentation.accessibilityTraits !== undefined
      ? { "data-swift-accessibility-traits": presentation.accessibilityTraits }
      : {}),
    ...(presentation.accessibilityElementChildren !== undefined
      ? {
          "data-swift-accessibility-element-children":
            presentation.accessibilityElementChildren,
        }
      : {}),
    ...(presentation.hasAccessibilityAction
      ? { "data-swift-accessibility-action": presentation.accessibilityActionName ?? "default" }
      : {}),
    ...(presentation.hasAccessibilityAction
      ? { "data-swift-accessibility-action-enabled": "true" }
      : {}),
    ...(presentation.accessibilityActivationPoint !== undefined
      ? {
          "data-swift-accessibility-activation-point":
            presentation.accessibilityActivationPoint,
        }
      : {}),
    ...(presentation.accessibilityActivationPointX !== undefined
      ? {
          "data-swift-accessibility-activation-point-x":
            presentation.accessibilityActivationPointX,
        }
      : {}),
    ...(presentation.accessibilityActivationPointY !== undefined
      ? {
          "data-swift-accessibility-activation-point-y":
            presentation.accessibilityActivationPointY,
        }
      : {}),
    ...(presentation.accessibilitySortPriority !== undefined
      ? { "data-swift-accessibility-sort-priority": presentation.accessibilitySortPriority }
      : {}),
    ...(presentation.animationName !== undefined
      ? { "data-swift-animation": presentation.animationName }
      : {}),
    ...(presentation.animationValue !== undefined
      ? { "data-swift-animation-value": presentation.animationValue }
      : {}),
    ...(presentation.alignmentGuide !== undefined
      ? { "data-swift-alignment-guide": presentation.alignmentGuide }
      : {}),
    ...(presentation.alignmentGuideOffset !== undefined
      ? { "data-swift-alignment-guide-offset": presentation.alignmentGuideOffset }
      : {}),
    ...(presentation.hasVisualEffect ? { "data-swift-visual-effect": "true" } : {}),
    ...(presentation.transitionName !== undefined
      ? { "data-swift-transition": presentation.transitionName }
      : {}),
    ...(presentation.contentTransitionName !== undefined
      ? { "data-swift-content-transition": presentation.contentTransitionName }
      : {}),
    ...(presentation.contentShape !== undefined
      ? { "data-swift-content-shape": presentation.contentShape }
      : {}),
    ...(presentation.safeAreaPaddingEdges !== undefined
      ? { "data-swift-safe-area-padding-edges": presentation.safeAreaPaddingEdges }
      : {}),
    ...(presentation.safeAreaPaddingLength !== undefined
      ? { "data-swift-safe-area-padding-length": presentation.safeAreaPaddingLength }
      : {}),
    ...(presentation.safeAreaPaddingInsets !== undefined
      ? { "data-swift-safe-area-padding-insets": presentation.safeAreaPaddingInsets }
      : {}),
    ...(presentation.contentMarginsEdges !== undefined
      ? { "data-swift-content-margins-edges": presentation.contentMarginsEdges }
      : {}),
    ...(presentation.contentMarginsLength !== undefined
      ? { "data-swift-content-margins-length": presentation.contentMarginsLength }
      : {}),
    ...(presentation.contentMarginsPlacement !== undefined
      ? { "data-swift-content-margins-placement": presentation.contentMarginsPlacement }
      : {}),
    ...(presentation.contentMarginsInsets !== undefined
      ? { "data-swift-content-margins-insets": presentation.contentMarginsInsets }
      : {}),
    ...(presentation.clipShapeStyle !== undefined
      ? { "data-swift-clip-style": presentation.clipShapeStyle }
      : {}),
    ...(presentation.clipShapeAntialiased !== undefined
      ? { "data-swift-clip-antialiased": String(presentation.clipShapeAntialiased) }
      : {}),
    ...(presentation.allowsHitTesting !== undefined
      ? { "data-swift-allows-hit-testing": String(presentation.allowsHitTesting) }
      : {}),
    ...(presentation.hidden === true ? { "data-swift-hidden": "true" } : {}),
    ...(presentation.badge !== undefined ? { "data-swift-badge": presentation.badge } : {}),
    ...(presentation.hoverEffect !== undefined
      ? { "data-swift-hover-effect": presentation.hoverEffect }
      : {}),
    ...(presentation.hoverEffectEnabled !== undefined
      ? { "data-swift-hover-effect-enabled": String(presentation.hoverEffectEnabled) }
      : {}),
    ...(presentation.defaultHoverEffect !== undefined
      ? { "data-swift-default-hover-effect": presentation.defaultHoverEffect }
      : {}),
    ...(presentation.coordinateSpace !== undefined
      ? { "data-swift-coordinate-space": presentation.coordinateSpace }
      : {}),
    ...(presentation.containerRelativeFrameAxis !== undefined
      ? {
          "data-swift-container-relative-frame-axis":
            presentation.containerRelativeFrameAxis,
        }
      : {}),
    ...(presentation.containerRelativeFrameCount !== undefined
      ? {
          "data-swift-container-relative-frame-count":
            presentation.containerRelativeFrameCount,
        }
      : {}),
    ...(presentation.containerRelativeFrameSpan !== undefined
      ? {
          "data-swift-container-relative-frame-span":
            presentation.containerRelativeFrameSpan,
        }
      : {}),
    ...(presentation.containerRelativeFrameSpacing !== undefined
      ? {
          "data-swift-container-relative-frame-spacing":
            presentation.containerRelativeFrameSpacing,
        }
      : {}),
    ...(presentation.containerRelativeFrameAlignment !== undefined
      ? {
          "data-swift-container-relative-frame-alignment":
            presentation.containerRelativeFrameAlignment,
        }
      : {}),
    ...(presentation.dynamicTypeSize !== undefined
      ? { "data-swift-dynamic-type-size": presentation.dynamicTypeSize }
      : {}),
    ...(presentation.preferredColorScheme !== undefined
      ? { "data-swift-preferred-color-scheme": presentation.preferredColorScheme }
      : {}),
    ...(presentation.environmentColorScheme !== undefined
      ? { "data-swift-environment-color-scheme": presentation.environmentColorScheme }
      : {}),
    ...(presentation.environmentLayoutDirection !== undefined
      ? { "data-swift-environment-layout-direction": presentation.environmentLayoutDirection }
      : {}),
    ...(presentation.flipsForRightToLeftLayoutDirection !== undefined
      ? {
          "data-swift-flips-for-rtl": String(
            presentation.flipsForRightToLeftLayoutDirection,
          ),
        }
      : {}),
    ...(presentation.redacted ? { "data-swift-redacted": "true" } : {}),
    ...(presentation.redactionReason !== undefined
      ? { "data-swift-redaction-reason": presentation.redactionReason }
      : {}),
    ...(presentation.privacySensitive ? { "data-swift-privacy-sensitive": "true" } : {}),
    ...(presentation.unredacted ? { "data-swift-unredacted": "true" } : {}),
    ...(presentation.gridCellAnchor !== undefined
      ? { "data-swift-grid-cell-anchor": presentation.gridCellAnchor }
      : {}),
    ...(presentation.gridCellColumns !== undefined
      ? { "data-swift-grid-cell-columns": presentation.gridCellColumns }
      : {}),
    ...(presentation.gridColumnAlignment !== undefined
      ? { "data-swift-grid-column-alignment": presentation.gridColumnAlignment }
      : {}),
    ...(presentation.scrollTargetBehavior !== undefined
      ? { "data-swift-scroll-target-behavior": presentation.scrollTargetBehavior }
      : {}),
    ...(presentation.scrollTargetLayout !== undefined
      ? { "data-swift-scroll-target-layout": presentation.scrollTargetLayout }
      : {}),
    ...(presentation.scrollBounceBehavior !== undefined
      ? { "data-swift-scroll-bounce-behavior": presentation.scrollBounceBehavior }
      : {}),
    ...(presentation.scrollBounceAxes !== undefined
      ? { "data-swift-scroll-bounce-axes": presentation.scrollBounceAxes }
      : {}),
    ...(presentation.scrollDisabled !== undefined
      ? { "data-swift-scroll-disabled": String(presentation.scrollDisabled) }
      : {}),
    ...(presentation.scrollPositionId !== undefined
      ? { "data-swift-scroll-position-id": presentation.scrollPositionId }
      : {}),
    ...(presentation.scrollPositionAnchor !== undefined
      ? { "data-swift-scroll-position-anchor": presentation.scrollPositionAnchor }
      : {}),
    ...(presentation.scrollPositionBinding !== undefined
      ? { "data-swift-scroll-position-binding": presentation.scrollPositionBinding }
      : {}),
    ...(presentation.defaultScrollAnchor !== undefined
      ? { "data-swift-default-scroll-anchor": presentation.defaultScrollAnchor }
      : {}),
    ...(presentation.tabViewStyle !== undefined
      ? { "data-swift-tab-view-style": presentation.tabViewStyle }
      : {}),
    ...(presentation.shapeStroke !== undefined
      ? { "data-swift-shape-stroke": presentation.shapeStroke }
      : {}),
    ...(presentation.shapeStrokeColor !== undefined
      ? { "data-swift-shape-stroke-color": presentation.shapeStrokeColor }
      : {}),
    ...(presentation.shapeStrokeWidth !== undefined
      ? { "data-swift-shape-stroke-width": presentation.shapeStrokeWidth }
      : {}),
    ...(presentation.labelsHidden === true ? { "data-swift-labels-hidden": "true" } : {}),
    ...(presentation.controlGroupStyle !== undefined
      ? { "data-swift-control-group-style": presentation.controlGroupStyle }
      : {}),
    ...(presentation.groupBoxStyle !== undefined
      ? { "data-swift-group-box-style": presentation.groupBoxStyle }
      : {}),
    ...(presentation.isFocusable ? { "data-swift-focusable": "true" } : {}),
    ...(presentation.focusedValue !== undefined
      ? { "data-swift-focused": String(presentation.focusedValue) }
      : {}),
    ...(presentation.focusedStateBindingKey !== undefined
      ? { "data-swift-focused-binding": presentation.focusedStateBindingKey }
      : {}),
    ...(presentation.symbolEffectName !== undefined
      ? { "data-swift-symbol-effect": presentation.symbolEffectName }
      : {}),
    ...(presentation.symbolEffectValue !== undefined
      ? { "data-swift-symbol-effect-value": presentation.symbolEffectValue }
      : {}),
    ...(presentation.symbolEffectActive !== undefined
      ? { "data-swift-symbol-effect-active": presentation.symbolEffectActive }
      : {}),
    ...(presentation.symbolEffectsRemoved === true
      ? { "data-swift-symbol-effects-removed": "true" }
      : {}),
    ...(presentation.hasOnAppear ? { "data-swift-on-appear": "true" } : {}),
    ...(presentation.hasOnDisappear ? { "data-swift-on-disappear": "true" } : {}),
    ...(presentation.hasOnHover ? { "data-swift-on-hover": "true" } : {}),
    ...(presentation.hasOnGeometryChange
      ? { "data-swift-on-geometry-change": "true" }
      : {}),
    ...(presentation.onGeometryChangeType !== undefined
      ? { "data-swift-on-geometry-change-type": presentation.onGeometryChangeType }
      : {}),
    ...(presentation.hasTask ? { "data-swift-task": "true" } : {}),
    ...(presentation.taskId !== undefined ? { "data-swift-task-id": presentation.taskId } : {}),
    ...(presentation.taskIdExpression !== undefined
      ? { "data-swift-task-id-expression": presentation.taskIdExpression }
      : {}),
    ...(presentation.identityValue !== undefined ? { "data-swift-id": presentation.identityValue } : {}),
    ...(presentation.dropDestinationType !== undefined
      ? { "data-swift-drop-destination": presentation.dropDestinationType }
      : {}),
    ...(presentation.draggableValue !== undefined
      ? { draggable: true, "data-swift-draggable": presentation.draggableValue }
      : {}),
  };
}

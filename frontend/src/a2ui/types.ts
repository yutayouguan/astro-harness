/** 前端 A2UI 操作与组件最小类型。 */

export type A2uiOperation = {
  version?: string;
  createSurface?: {
    surfaceId: string;
    catalogId?: string;
  };
  updateComponents?: {
    surfaceId: string;
    components: A2uiComponent[];
  };
  updateDataModel?: {
    surfaceId: string;
    path?: string;
    value?: unknown;
  };
  deleteSurface?: {
    surfaceId: string;
  };
};

export type A2uiComponent = {
  id: string;
  component: string;
  text?: string;
  variant?: string;
  child?: string;
  children?: string[];
  name?: string;
  src?: string;
  url?: string;
  label?: string;
  value?: string | boolean;
  hint?: string;
  size?: string;
  required?: boolean;
  options?: Array<string | { label?: string; value?: string }>;
  action?: {
    event?: {
      name?: string;
      context?: Record<string, unknown>;
    };
  };
  [key: string]: unknown;
};

export const ASTRO_CATALOG_ID = "astro://a2ui/catalog/v2";

export const ALLOWED_COMPONENTS = new Set([
  "Text",
  "Icon",
  "Divider",
  "Card",
  "Column",
  "Row",
  "Button",
  "TextField",
  "ChoicePicker",
  "CheckBox",
  "Image",
  "List",
  "Badge",
  "Chip",
  "Metric",
  "Avatar",
  "Callout",
  "Spacer",
  "ClarifyWizard",
]);

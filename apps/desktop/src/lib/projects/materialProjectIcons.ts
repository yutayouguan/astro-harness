import { materialIconManifest } from "../filespace/generated/materialIconManifest.ts";

export type MaterialProjectIcon = {
  id: string;
  label: string;
  searchText: string;
};

/** 品牌图标不走 material 抽取流程，单独放在 public/project-icons 下避免被清理。 */
export const ASTRO_SPACE_ICON_ID = "astro-space";

const BRAND_ICON_DIR = "/project-icons/";
const MATERIAL_ICON_DIR = "/file-icons/";

const BRAND_ICONS: MaterialProjectIcon[] = [
  {
    id: ASTRO_SPACE_ICON_ID,
    label: "主空间",
    searchText: "astro space 主空间 logo brand 品牌 默认",
  },
];

const expandedByClosed = new Map<string, string>();
for (const [folderName, closedId] of Object.entries(materialIconManifest.folderNames)) {
  const expandedId = materialIconManifest.folderNamesExpanded[folderName];
  if (expandedId && !expandedByClosed.has(closedId)) {
    expandedByClosed.set(closedId, expandedId);
  }
}
expandedByClosed.set(
  materialIconManifest.defaults.folder,
  materialIconManifest.defaults.folderExpanded,
);

const aliasesByIcon = new Map<string, Set<string>>();
for (const [folderName, iconId] of Object.entries(materialIconManifest.folderNames)) {
  const aliases = aliasesByIcon.get(iconId) ?? new Set<string>();
  aliases.add(folderName.replace(/[_-]/g, " "));
  aliasesByIcon.set(iconId, aliases);
}

const PINNED_ICON_IDS = [
  "folder",
  "folder-home",
  "folder-project",
  "folder-src",
  "folder-code",
  "folder-app",
  "folder-api",
  "folder-ui",
  "folder-tools",
  "folder-robot",
  "folder-atom",
  "folder-cursor",
  "folder-claude",
  "folder-gemini-ai",
  "folder-typescript",
  "folder-rust",
  "folder-python",
  "folder-react-components",
  "folder-database",
  "folder-cloud",
  "folder-docs",
  "folder-test",
];

const labelFor = (id: string) =>
  id
    .replace(/^folder-?/, "")
    .split("-")
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ") || "Folder";

const allIds = new Set<string>([
  materialIconManifest.defaults.folder,
  ...Object.values(materialIconManifest.folderNames),
]);

export const MATERIAL_PROJECT_ICONS: MaterialProjectIcon[] = [
  ...BRAND_ICONS,
  ...[...allIds]
    .map((id) => {
      const aliases = [...(aliasesByIcon.get(id) ?? [])];
      const label = labelFor(id);
      return {
        id,
        label,
        searchText: `${id} ${label} ${aliases.join(" ")}`.toLowerCase(),
      };
    })
    .sort((a, b) => {
      const aPinned = PINNED_ICON_IDS.indexOf(a.id);
      const bPinned = PINNED_ICON_IDS.indexOf(b.id);
      if (aPinned >= 0 || bPinned >= 0) {
        if (aPinned < 0) return 1;
        if (bPinned < 0) return -1;
        return aPinned - bPinned;
      }
      return a.label.localeCompare(b.label);
    }),
];

const brandIds = new Set(BRAND_ICONS.map((icon) => icon.id));
const knownIds = new Set(MATERIAL_PROJECT_ICONS.map((icon) => icon.id));

export function isMaterialProjectIcon(iconId: string | null | undefined): boolean {
  return Boolean(iconId && knownIds.has(iconId));
}

export function materialProjectIconUrl(
  iconId: string | null | undefined,
  expanded: boolean,
): string {
  const closedId =
    iconId && knownIds.has(iconId) ? iconId : materialIconManifest.defaults.folder;
  if (brandIds.has(closedId)) {
    return `${BRAND_ICON_DIR}${closedId}${expanded ? "-open" : ""}.svg`;
  }
  const resolvedId = expanded ? (expandedByClosed.get(closedId) ?? closedId) : closedId;
  return `${MATERIAL_ICON_DIR}${resolvedId}.svg`;
}

export function filterMaterialProjectIcons(query: string): MaterialProjectIcon[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return MATERIAL_PROJECT_ICONS;
  return MATERIAL_PROJECT_ICONS.filter((icon) => icon.searchText.includes(normalized));
}

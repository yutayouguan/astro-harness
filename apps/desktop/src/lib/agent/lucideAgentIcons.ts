/** Lucide Agent 图标目录与 SVG base64 渲染。 */
import { createElement, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import {
  AlarmClock as AlarmClockData,
  Anchor as AnchorData,
  Aperture as ApertureData,
  Apple as AppleData,
  Archive as ArchiveData,
  ArrowRightFromLine as ArrowRightFromLineData,
  ArrowUpDown as ArrowUpDownData,
  Atom as AtomData,
  AudioLines as AudioLinesData,
  AudioWaveform as AudioWaveformData,
  Award as AwardData,
  BadgeCheck as BadgeCheckData,
  Banknote as BanknoteData,
  BarChart3 as BarChart3Data,
  BatteryCharging as BatteryChargingData,
  Bell as BellData,
  Bike as BikeData,
  Binoculars as BinocularsData,
  Bird as BirdData,
  Bluetooth as BluetoothData,
  Bolt as BoltData,
  BookMarked as BookMarkedData,
  BookOpen as BookOpenData,
  Bookmark as BookmarkData,
  Bot as BotData,
  Box as BoxData,
  Boxes as BoxesData,
  Brain as BrainData,
  Braces as BracesData,
  Briefcase as BriefcaseData,
  Brush as BrushData,
  Bug as BugData,
  Building2 as Building2Data,
  Bus as BusData,
  Calculator as CalculatorData,
  Calendar as CalendarData,
  Camera as CameraData,
  Captions as CaptionsData,
  Car as CarData,
  Cat as CatData,
  ChartColumn as ChartColumnData,
  ChartPie as ChartPieData,
  CheckCircle2 as CheckCircle2Data,
  Clapperboard as ClapperboardData,
  ClipboardList as ClipboardListData,
  Clock as ClockData,
  Cloud as CloudData,
  Code as CodeData,
  CloudSun as CloudSunData,
  Code2 as Code2Data,
  Coffee as CoffeeData,
  Cog as CogData,
  Compass as CompassData,
  Component as ComponentData,
  Contact as ContactData,
  Cookie as CookieData,
  Cpu as CpuData,
  CreditCard as CreditCardData,
  Crown as CrownData,
  Database as DatabaseData,
  Dog as DogData,
  Drama as DramaData,
  Droplets as DropletsData,
  Dumbbell as DumbbellData,
  Ear as EarData,
  Earth as EarthData,
  Egg as EggData,
  Eye as EyeData,
  Factory as FactoryData,
  Feather as FeatherData,
  FileCode2 as FileCode2Data,
  FileJson as FileJsonData,
  FileSearch as FileSearchData,
  FileText as FileTextData,
  Film as FilmData,
  Filter as FilterData,
  Fingerprint as FingerprintData,
  FireExtinguisher as FireExtinguisherData,
  Fish as FishData,
  Flag as FlagData,
  Flame as FlameData,
  FlaskConical as FlaskConicalData,
  Flower2 as Flower2Data,
  FolderKanban as FolderKanbanData,
  Footprints as FootprintsData,
  Gamepad2 as Gamepad2Data,
  Gauge as GaugeData,
  Gem as GemData,
  Ghost as GhostData,
  Gift as GiftData,
  GitBranch as GitBranchData,
  GitFork as GitForkData,
  GitMerge as GitMergeData,
  Github as GithubData,
  Globe as GlobeData,
  Glasses as GlassesData,
  Globe2 as Globe2Data,
  GraduationCap as GraduationCapData,
  Grape as GrapeData,
  Hand as HandData,
  Hammer as HammerData,
  HandHeart as HandHeartData,
  Headphones as HeadphonesData,
  Heart as HeartData,
  HeartHandshake as HeartHandshakeData,
  Home as HomeData,
  Hourglass as HourglassData,
  Image as ImageData,
  Inbox as InboxData,
  KeyRound as KeyRoundData,
  Landmark as LandmarkData,
  Languages as LanguagesData,
  Laptop as LaptopData,
  Laugh as LaughData,
  Layers as LayersData,
  LayoutDashboard as LayoutDashboardData,
  Leaf as LeafData,
  Library as LibraryData,
  Lightbulb as LightbulbData,
  Link2 as Link2Data,
  ListTodo as ListTodoData,
  Lock as LockData,
  Mail as MailData,
  Map as MapData,
  MapPin as MapPinData,
  Medal as MedalData,
  Megaphone as MegaphoneData,
  MessageCircle as MessageCircleData,
  MessagesSquare as MessagesSquareData,
  Mic as MicData,
  Microscope as MicroscopeData,
  Milestone as MilestoneData,
  MonitorSmartphone as MonitorSmartphoneData,
  Moon as MoonData,
  Mountain as MountainData,
  MousePointer2 as MousePointer2Data,
  MousePointerClick as MousePointerClickData,
  Music as MusicData,
  Navigation as NavigationData,
  Newspaper as NewspaperData,
  NotebookPen as NotebookPenData,
  Orbit as OrbitData,
  Package as PackageData,
  Paintbrush as PaintbrushData,
  Palette as PaletteData,
  Paperclip as PaperclipData,
  PartyPopper as PartyPopperData,
  PenLine as PenLineData,
  PencilRuler as PencilRulerData,
  Phone as PhoneData,
  PieChart as PieChartData,
  PiggyBank as PiggyBankData,
  Plane as PlaneData,
  Play as PlayData,
  Plug as PlugData,
  PocketKnife as PocketKnifeData,
  Presentation as PresentationData,
  Printer as PrinterData,
  Puzzle as PuzzleData,
  Quote as QuoteData,
  Radio as RadioData,
  Rainbow as RainbowData,
  Recycle as RecycleData,
  Repeat as RepeatData,
  Rocket as RocketData,
  Route as RouteData,
  Sailboat as SailboatData,
  Scale as ScaleData,
  ScanFace as ScanFaceData,
  School as SchoolData,
  Scissors as ScissorsData,
  Search as SearchData,
  Send as SendData,
  Server as ServerData,
  Settings2 as Settings2Data,
  Share2 as Share2Data,
  Shell as ShellData,
  Shield as ShieldData,
  Ship as ShipData,
  Sigma as SigmaData,
  ShoppingBag as ShoppingBagData,
  Shuffle as ShuffleData,
  Signal as SignalData,
  Skull as SkullData,
  Smile as SmileData,
  Snowflake as SnowflakeData,
  Sofa as SofaData,
  Sparkles as SparklesData,
  Speaker as SpeakerData,
  Sprout as SproutData,
  Star as StarData,
  Stethoscope as StethoscopeData,
  Store as StoreData,
  Sun as SunData,
  Sunrise as SunriseData,
  Sword as SwordData,
  Table2 as Table2Data,
  TabletSmartphone as TabletSmartphoneData,
  Tag as TagData,
  Target as TargetData,
  Tent as TentData,
  Terminal as TerminalData,
  Thermometer as ThermometerData,
  Timer as TimerData,
  TrainFront as TrainFrontData,
  Trash2 as Trash2Data,
  Trees as TreesData,
  Trophy as TrophyData,
  Truck as TruckData,
  Tv as TvData,
  Type as TypeData,
  Umbrella as UmbrellaData,
  University as UniversityData,
  UserCheck as UserCheckData,
  UserRound as UserRoundData,
  Users as UsersData,
  Utensils as UtensilsData,
  Video as VideoData,
  Volleyball as VolleyballData,
  Wallet as WalletData,
  WandSparkles as WandSparklesData,
  Warehouse as WarehouseData,
  Watch as WatchData,
  Waves as WavesData,
  Webcam as WebcamData,
  Webhook as WebhookData,
  Wifi as WifiData,
  Wind as WindData,
  Wine as WineData,
  Workflow as WorkflowData,
  Wrench as WrenchData,
  Zap as ZapData,
  type IconNode as LucideDataNode,
} from "lucide";
import {
  AlarmClock,
  Anchor,
  Aperture,
  Apple,
  Archive,
  ArrowRightFromLine,
  ArrowUpDown,
  Atom,
  AudioLines,
  AudioWaveform,
  Award,
  BadgeCheck,
  Banknote,
  BarChart3,
  BatteryCharging,
  Bell,
  Bike,
  Binoculars,
  Bird,
  Bluetooth,
  Bolt,
  BookMarked,
  BookOpen,
  Bookmark,
  Bot,
  Box,
  Boxes,
  Brain,
  Braces,
  Briefcase,
  Brush,
  Bug,
  Building2,
  Bus,
  Calculator,
  Calendar,
  Camera,
  Captions,
  Car,
  Cat,
  ChartColumn,
  ChartPie,
  CheckCircle2,
  Clapperboard,
  ClipboardList,
  Clock,
  Cloud,
  Code,
  CloudSun,
  Code2,
  Coffee,
  Cog,
  Compass,
  Component,
  Contact,
  Cookie,
  Cpu,
  CreditCard,
  Crown,
  Database,
  Dog,
  Drama,
  Droplets,
  Dumbbell,
  Ear,
  Earth,
  Egg,
  Eye,
  Factory,
  Feather,
  FileCode2,
  FileJson,
  FileSearch,
  FileText,
  Film,
  Filter,
  Fingerprint,
  FireExtinguisher,
  Fish,
  Flag,
  Flame,
  FlaskConical,
  Flower2,
  FolderKanban,
  Footprints,
  Gamepad2,
  Gauge,
  Gem,
  Ghost,
  Gift,
  GitBranch,
  GitFork,
  GitMerge,
  Github,
  Globe,
  Glasses,
  Globe2,
  GraduationCap,
  Grape,
  Hand,
  Hammer,
  HandHeart,
  Headphones,
  Heart,
  HeartHandshake,
  Home,
  Hourglass,
  Image,
  Inbox,
  KeyRound,
  Landmark,
  Languages,
  Laptop,
  Laugh,
  Layers,
  LayoutDashboard,
  Leaf,
  Library,
  Lightbulb,
  Link2,
  ListTodo,
  Lock,
  Mail,
  Map,
  MapPin,
  Medal,
  Megaphone,
  MessageCircle,
  MessagesSquare,
  Mic,
  Microscope,
  Milestone,
  MonitorSmartphone,
  Moon,
  Mountain,
  MousePointer2,
  MousePointerClick,
  Music,
  Navigation,
  Newspaper,
  NotebookPen,
  Orbit,
  Package,
  Paintbrush,
  Palette,
  Paperclip,
  PartyPopper,
  PenLine,
  PencilRuler,
  Phone,
  PieChart,
  PiggyBank,
  Plane,
  Play,
  Plug,
  PocketKnife,
  Presentation,
  Printer,
  Puzzle,
  Quote,
  Radio,
  Rainbow,
  Recycle,
  Repeat,
  Rocket,
  Route,
  Sailboat,
  Scale,
  ScanFace,
  School,
  Scissors,
  Search,
  Send,
  Server,
  Settings2,
  Share2,
  Shell,
  Shield,
  Ship,
  Sigma,
  ShoppingBag,
  Shuffle,
  Signal,
  Skull,
  Smile,
  Snowflake,
  Sofa,
  Sparkles,
  Speaker,
  Sprout,
  Star,
  Stethoscope,
  Store,
  Sun,
  Sunrise,
  Sword,
  Table2,
  TabletSmartphone,
  Tag,
  Target,
  Tent,
  Terminal,
  Thermometer,
  Timer,
  TrainFront,
  Trash2,
  Trees,
  Trophy,
  Truck,
  Tv,
  Type,
  Umbrella,
  University,
  UserCheck,
  UserRound,
  Users,
  Utensils,
  Video,
  Volleyball,
  Wallet,
  WandSparkles,
  Warehouse,
  Watch,
  Waves,
  Webcam,
  Webhook,
  Wifi,
  Wind,
  Wine,
  Workflow,
  Wrench,
  Zap,
  type LucideProps,
} from "lucide-react";

const LUCIDE_DATA_BY_NAME: Record<string, LucideDataNode> = {
  AlarmClock: AlarmClockData,
  Anchor: AnchorData,
  Aperture: ApertureData,
  Apple: AppleData,
  Archive: ArchiveData,
  ArrowRightFromLine: ArrowRightFromLineData,
  ArrowUpDown: ArrowUpDownData,
  Atom: AtomData,
  AudioLines: AudioLinesData,
  AudioWaveform: AudioWaveformData,
  Award: AwardData,
  BadgeCheck: BadgeCheckData,
  Banknote: BanknoteData,
  BarChart3: BarChart3Data,
  BatteryCharging: BatteryChargingData,
  Bell: BellData,
  Bike: BikeData,
  Binoculars: BinocularsData,
  Bird: BirdData,
  Bluetooth: BluetoothData,
  Bolt: BoltData,
  BookMarked: BookMarkedData,
  BookOpen: BookOpenData,
  Bookmark: BookmarkData,
  Bot: BotData,
  Box: BoxData,
  Boxes: BoxesData,
  Brain: BrainData,
  Braces: BracesData,
  Briefcase: BriefcaseData,
  Brush: BrushData,
  Bug: BugData,
  Building2: Building2Data,
  Bus: BusData,
  Calculator: CalculatorData,
  Calendar: CalendarData,
  Camera: CameraData,
  Captions: CaptionsData,
  Car: CarData,
  Cat: CatData,
  ChartColumn: ChartColumnData,
  ChartPie: ChartPieData,
  CheckCircle2: CheckCircle2Data,
  Clapperboard: ClapperboardData,
  ClipboardList: ClipboardListData,
  Clock: ClockData,
  Cloud: CloudData,
  Code: CodeData,
  CloudSun: CloudSunData,
  Code2: Code2Data,
  Coffee: CoffeeData,
  Cog: CogData,
  Compass: CompassData,
  Component: ComponentData,
  Contact: ContactData,
  Cookie: CookieData,
  Cpu: CpuData,
  CreditCard: CreditCardData,
  Crown: CrownData,
  Database: DatabaseData,
  Dog: DogData,
  Drama: DramaData,
  Droplets: DropletsData,
  Dumbbell: DumbbellData,
  Ear: EarData,
  Earth: EarthData,
  Egg: EggData,
  Eye: EyeData,
  Factory: FactoryData,
  Feather: FeatherData,
  FileCode2: FileCode2Data,
  FileJson: FileJsonData,
  FileSearch: FileSearchData,
  FileText: FileTextData,
  Film: FilmData,
  Filter: FilterData,
  Fingerprint: FingerprintData,
  FireExtinguisher: FireExtinguisherData,
  Fish: FishData,
  Flag: FlagData,
  Flame: FlameData,
  FlaskConical: FlaskConicalData,
  Flower2: Flower2Data,
  FolderKanban: FolderKanbanData,
  Footprints: FootprintsData,
  Gamepad2: Gamepad2Data,
  Gauge: GaugeData,
  Gem: GemData,
  Ghost: GhostData,
  Gift: GiftData,
  GitBranch: GitBranchData,
  GitFork: GitForkData,
  GitMerge: GitMergeData,
  Github: GithubData,
  Globe: GlobeData,
  Glasses: GlassesData,
  Globe2: Globe2Data,
  GraduationCap: GraduationCapData,
  Grape: GrapeData,
  Hand: HandData,
  Hammer: HammerData,
  HandHeart: HandHeartData,
  Headphones: HeadphonesData,
  Heart: HeartData,
  HeartHandshake: HeartHandshakeData,
  Home: HomeData,
  Hourglass: HourglassData,
  Image: ImageData,
  Inbox: InboxData,
  KeyRound: KeyRoundData,
  Landmark: LandmarkData,
  Languages: LanguagesData,
  Laptop: LaptopData,
  Laugh: LaughData,
  Layers: LayersData,
  LayoutDashboard: LayoutDashboardData,
  Leaf: LeafData,
  Library: LibraryData,
  Lightbulb: LightbulbData,
  Link2: Link2Data,
  ListTodo: ListTodoData,
  Lock: LockData,
  Mail: MailData,
  Map: MapData,
  MapPin: MapPinData,
  Medal: MedalData,
  Megaphone: MegaphoneData,
  MessageCircle: MessageCircleData,
  MessagesSquare: MessagesSquareData,
  Mic: MicData,
  Microscope: MicroscopeData,
  Milestone: MilestoneData,
  MonitorSmartphone: MonitorSmartphoneData,
  Moon: MoonData,
  Mountain: MountainData,
  MousePointer2: MousePointer2Data,
  MousePointerClick: MousePointerClickData,
  Music: MusicData,
  Navigation: NavigationData,
  Newspaper: NewspaperData,
  NotebookPen: NotebookPenData,
  Orbit: OrbitData,
  Package: PackageData,
  Paintbrush: PaintbrushData,
  Palette: PaletteData,
  Paperclip: PaperclipData,
  PartyPopper: PartyPopperData,
  PenLine: PenLineData,
  PencilRuler: PencilRulerData,
  Phone: PhoneData,
  PieChart: PieChartData,
  PiggyBank: PiggyBankData,
  Plane: PlaneData,
  Play: PlayData,
  Plug: PlugData,
  PocketKnife: PocketKnifeData,
  Presentation: PresentationData,
  Printer: PrinterData,
  Puzzle: PuzzleData,
  Quote: QuoteData,
  Radio: RadioData,
  Rainbow: RainbowData,
  Recycle: RecycleData,
  Repeat: RepeatData,
  Rocket: RocketData,
  Route: RouteData,
  Sailboat: SailboatData,
  Scale: ScaleData,
  ScanFace: ScanFaceData,
  School: SchoolData,
  Scissors: ScissorsData,
  Search: SearchData,
  Send: SendData,
  Server: ServerData,
  Settings2: Settings2Data,
  Share2: Share2Data,
  Shell: ShellData,
  Shield: ShieldData,
  Ship: ShipData,
  Sigma: SigmaData,
  ShoppingBag: ShoppingBagData,
  Shuffle: ShuffleData,
  Signal: SignalData,
  Skull: SkullData,
  Smile: SmileData,
  Snowflake: SnowflakeData,
  Sofa: SofaData,
  Sparkles: SparklesData,
  Speaker: SpeakerData,
  Sprout: SproutData,
  Star: StarData,
  Stethoscope: StethoscopeData,
  Store: StoreData,
  Sun: SunData,
  Sunrise: SunriseData,
  Sword: SwordData,
  Table2: Table2Data,
  TabletSmartphone: TabletSmartphoneData,
  Tag: TagData,
  Target: TargetData,
  Tent: TentData,
  Terminal: TerminalData,
  Thermometer: ThermometerData,
  Timer: TimerData,
  TrainFront: TrainFrontData,
  Trash2: Trash2Data,
  Trees: TreesData,
  Trophy: TrophyData,
  Truck: TruckData,
  Tv: TvData,
  Type: TypeData,
  Umbrella: UmbrellaData,
  University: UniversityData,
  UserCheck: UserCheckData,
  UserRound: UserRoundData,
  Users: UsersData,
  Utensils: UtensilsData,
  Video: VideoData,
  Volleyball: VolleyballData,
  Wallet: WalletData,
  WandSparkles: WandSparklesData,
  Warehouse: WarehouseData,
  Watch: WatchData,
  Waves: WavesData,
  Webcam: WebcamData,
  Webhook: WebhookData,
  Wifi: WifiData,
  Wind: WindData,
  Wine: WineData,
  Workflow: WorkflowData,
  Wrench: WrenchData,
  Zap: ZapData,
  // lucide-react canonicalizes these legacy aliases in displayName.
  CodeXml: Code2Data,
  House: HomeData,
  CircleCheck: CheckCircle2Data,
};

export type LucideIconComponent = ComponentType<LucideProps>;

export type LucideAgentIcon = {
  id: string;
  label: string;
  Icon: LucideIconComponent;
  data: LucideDataNode;
  keywords: string[];
};

export const LUCIDE_ICON_COLORS = [
  { id: "slate", label: "Slate", value: "#0f172a" },
  { id: "blue", label: "Blue", value: "#2563eb" },
  { id: "cyan", label: "Cyan", value: "#0891b2" },
  { id: "teal", label: "Teal", value: "#0d9488" },
  { id: "green", label: "Green", value: "#16a34a" },
  { id: "lime", label: "Lime", value: "#65a30d" },
  { id: "amber", label: "Amber", value: "#d97706" },
  { id: "orange", label: "Orange", value: "#ea580c" },
  { id: "red", label: "Red", value: "#dc2626" },
  { id: "rose", label: "Rose", value: "#e11d48" },
  { id: "pink", label: "Pink", value: "#db2777" },
  { id: "purple", label: "Purple", value: "#7c3aed" },
  { id: "violet", label: "Violet", value: "#8b5cf6" },
  { id: "indigo", label: "Indigo", value: "#4f46e5" },
] as const;

export const LUCIDE_ICON_GRADIENTS = [
  { id: "ocean", label: "Ocean", from: "#2563eb", to: "#06b6d4", angle: 135 },
  { id: "sunset", label: "Sunset", from: "#f97316", to: "#ef4444", angle: 135 },
  { id: "aurora", label: "Aurora", from: "#8b5cf6", to: "#22d3ee", angle: 135 },
  { id: "mango", label: "Mango", from: "#f59e0b", to: "#ef4444", angle: 120 },
  { id: "mint", label: "Mint", from: "#10b981", to: "#06b6d4", angle: 135 },
  { id: "berry", label: "Berry", from: "#db2777", to: "#7c3aed", angle: 135 },
  { id: "flame", label: "Flame", from: "#ef4444", to: "#eab308", angle: 135 },
  { id: "forest", label: "Forest", from: "#15803d", to: "#84cc16", angle: 135 },
  { id: "night", label: "Night", from: "#1e3a8a", to: "#7c3aed", angle: 135 },
  { id: "peach", label: "Peach", from: "#fb7185", to: "#fb923c", angle: 135 },
  { id: "sky", label: "Sky", from: "#38bdf8", to: "#6366f1", angle: 135 },
  { id: "limeade", label: "Limeade", from: "#84cc16", to: "#14b8a6", angle: 135 },
] as const;

export type LucidePaint =
  | { kind: "solid"; color: string }
  | { kind: "gradient"; id: string; from: string; to: string; angle: number };

/** 描边：线条着色；填充镂空：颜色/渐变填入图形，空洞保持透明 */
export type LucideRenderStyle = "stroke" | "fillCutout";

export const DEFAULT_LUCIDE_ICON_COLOR = LUCIDE_ICON_COLORS[0].value;

export const DEFAULT_LUCIDE_PAINT: LucidePaint = {
  kind: "solid",
  color: DEFAULT_LUCIDE_ICON_COLOR,
};

export const DEFAULT_LUCIDE_RENDER_STYLE: LucideRenderStyle = "stroke";

export function solidPaint(color: string): LucidePaint {
  return { kind: "solid", color };
}

export function gradientPaint(
  g: { id: string; from: string; to: string; angle: number },
): LucidePaint {
  return { kind: "gradient", id: g.id, from: g.from, to: g.to, angle: g.angle };
}

export function paintCssBackground(paint: LucidePaint): string {
  if (paint.kind === "solid") return paint.color;
  return `linear-gradient(${paint.angle}deg, ${paint.from}, ${paint.to})`;
}

export function paintsEqual(a: LucidePaint, b: LucidePaint): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "solid" && b.kind === "solid") return a.color === b.color;
  if (a.kind === "gradient" && b.kind === "gradient") {
    return (
      a.from === b.from &&
      a.to === b.to &&
      a.angle === b.angle &&
      a.id === b.id
    );
  }
  return false;
}

function gradientCoords(angleDeg: number): { x1: string; y1: string; x2: string; y2: string } {
  const rad = ((angleDeg % 360) * Math.PI) / 180;
  const x = Math.cos(rad);
  const y = Math.sin(rad);
  return {
    x1: `${50 - x * 50}%`,
    y1: `${50 - y * 50}%`,
    x2: `${50 + x * 50}%`,
    y2: `${50 + y * 50}%`,
  };
}

function ensurePaintGradient(defs: SVGDefsElement, paint: LucidePaint, gradId: string): string {
  if (paint.kind === "solid") return paint.color;
  const ns = "http://www.w3.org/2000/svg";
  defs.querySelector(`#${gradId}`)?.remove();
  const lg = document.createElementNS(ns, "linearGradient");
  lg.setAttribute("id", gradId);
  const { x1, y1, x2, y2 } = gradientCoords(paint.angle);
  lg.setAttribute("x1", x1);
  lg.setAttribute("y1", y1);
  lg.setAttribute("x2", x2);
  lg.setAttribute("y2", y2);
  const stop1 = document.createElementNS(ns, "stop");
  stop1.setAttribute("offset", "0%");
  stop1.setAttribute("stop-color", paint.from);
  const stop2 = document.createElementNS(ns, "stop");
  stop2.setAttribute("offset", "100%");
  stop2.setAttribute("stop-color", paint.to);
  lg.appendChild(stop1);
  lg.appendChild(stop2);
  defs.appendChild(lg);
  return `url(#${gradId})`;
}

function ensureDefs(svg: SVGElement): SVGDefsElement {
  const ns = "http://www.w3.org/2000/svg";
  let defs = svg.querySelector(":scope > defs");
  if (!defs) {
    defs = document.createElementNS(ns, "defs");
    svg.insertBefore(defs, svg.firstChild);
  }
  return defs as SVGDefsElement;
}

function parseViewBox(svg: SVGElement): { x: number; y: number; w: number; h: number } {
  const raw = svg.getAttribute("viewBox");
  if (raw) {
    const parts = raw.trim().split(/[\s,]+/).map(Number);
    if (parts.length === 4 && parts.every((n) => Number.isFinite(n))) {
      return { x: parts[0], y: parts[1], w: parts[2], h: parts[3] };
    }
  }
  const w = Number(svg.getAttribute("width") || 24);
  const h = Number(svg.getAttribute("height") || 24);
  return { x: 0, y: 0, w, h };
}

function forceMaskTone(el: Element): void {
  const fill = el.getAttribute("fill");
  if (fill && fill !== "none" && fill !== "transparent") {
    el.setAttribute("fill", "#fff");
  }
  const stroke = el.getAttribute("stroke");
  if (stroke && stroke !== "none" && stroke !== "transparent") {
    el.setAttribute("stroke", "#fff");
  }
  el.childNodes.forEach((child) => {
    if (child.nodeType === 1) forceMaskTone(child as Element);
  });
}

/** 填充镂空：用图标轮廓做遮罩，颜色/渐变填入，空洞透明 */
function applyFillCutoutPaint(svg: SVGElement, paint: LucidePaint): void {
  const ns = "http://www.w3.org/2000/svg";
  const GRAD_ID = "astro-lucide-grad";
  const MASK_ID = "astro-lucide-mask";

  svg.querySelector(`[data-astro-fill-cutout]`)?.remove();
  svg.querySelector(`#${MASK_ID}`)?.remove();
  svg.querySelector(`#${GRAD_ID}`)?.remove();

  const vb = parseViewBox(svg);
  const drawables = Array.from(svg.children).filter(
    (el) => el.tagName.toLowerCase() !== "defs",
  );
  if (drawables.length === 0) return;

  const defs = ensureDefs(svg);
  const fillValue = ensurePaintGradient(defs, paint, GRAD_ID);

  const mask = document.createElementNS(ns, "mask");
  mask.setAttribute("id", MASK_ID);
  mask.setAttribute("maskUnits", "userSpaceOnUse");
  mask.setAttribute("x", String(vb.x));
  mask.setAttribute("y", String(vb.y));
  mask.setAttribute("width", String(vb.w));
  mask.setAttribute("height", String(vb.h));

  const black = document.createElementNS(ns, "rect");
  black.setAttribute("x", String(vb.x));
  black.setAttribute("y", String(vb.y));
  black.setAttribute("width", String(vb.w));
  black.setAttribute("height", String(vb.h));
  black.setAttribute("fill", "#000");
  mask.appendChild(black);

  const g = document.createElementNS(ns, "g");
  g.setAttribute("fill", "none");
  g.setAttribute("stroke", "#fff");
  g.setAttribute("stroke-width", svg.getAttribute("stroke-width") || "2");
  g.setAttribute("stroke-linecap", svg.getAttribute("stroke-linecap") || "round");
  g.setAttribute("stroke-linejoin", svg.getAttribute("stroke-linejoin") || "round");

  for (const el of drawables) {
    const clone = el.cloneNode(true) as Element;
    forceMaskTone(clone);
    if (!clone.getAttribute("stroke") || clone.getAttribute("stroke") === "none") {
      // Lucide 根上 stroke=currentColor，子路径常继承；保证白描边进遮罩
      clone.setAttribute("stroke", "#fff");
    }
    g.appendChild(clone);
  }
  mask.appendChild(g);
  defs.appendChild(mask);

  drawables.forEach((el) => el.remove());

  const rect = document.createElementNS(ns, "rect");
  rect.setAttribute("data-astro-fill-cutout", "1");
  rect.setAttribute("x", String(vb.x));
  rect.setAttribute("y", String(vb.y));
  rect.setAttribute("width", String(vb.w));
  rect.setAttribute("height", String(vb.h));
  rect.setAttribute("fill", fillValue);
  rect.setAttribute("mask", `url(#${MASK_ID})`);
  svg.appendChild(rect);

  svg.setAttribute("stroke", "none");
  svg.setAttribute("fill", "none");
}

/** 给已渲染的 Lucide SVG 注入纯色/渐变；style=fillCutout 时做镂空填充 */
export function applyPaintToSvg(
  svg: SVGElement,
  paint: LucidePaint,
  style: LucideRenderStyle = "stroke",
): void {
  const GRAD_ID = "astro-lucide-grad";
  // 清掉上次填充镂空残留
  svg.querySelector(`[data-astro-fill-cutout]`)?.remove();
  svg.querySelector(`#astro-lucide-mask`)?.remove();

  if (style === "fillCutout") {
    applyFillCutoutPaint(svg, paint);
    return;
  }

  svg.querySelector(`#${GRAD_ID}`)?.closest("defs")?.remove();

  const setStroke = (value: string) => {
    svg.setAttribute("stroke", value);
    svg.querySelectorAll("[stroke]").forEach((el) => {
      const cur = el.getAttribute("stroke");
      if (cur && cur !== "none" && cur !== "transparent") {
        el.setAttribute("stroke", value);
      }
    });
  };

  if (paint.kind === "solid") {
    setStroke(paint.color);
    svg.setAttribute("fill", "none");
    return;
  }

  const defs = ensureDefs(svg);
  const fillValue = ensurePaintGradient(defs, paint, GRAD_ID);
  setStroke(fillValue);
  svg.setAttribute("fill", "none");
}

/** 创建 Agent 时可选的 Lucide 图标 */
const LUCIDE_AGENT_ICON_COMPONENTS: Omit<LucideAgentIcon, "data">[] = [
  { id: "bot", label: "Bot", Icon: Bot, keywords: ["ai", "robot", "助手"] },
  { id: "brain", label: "Brain", Icon: Brain, keywords: ["ai", "思考", "智能"] },
  { id: "sparkles", label: "Sparkles", Icon: Sparkles, keywords: ["magic", "闪光"] },
  { id: "wand-sparkles", label: "Wand", Icon: WandSparkles, keywords: ["魔法", "生成"] },
  { id: "rocket", label: "Rocket", Icon: Rocket, keywords: ["启动", "增长"] },
  { id: "zap", label: "Zap", Icon: Zap, keywords: ["快", "能量"] },
  { id: "bolt", label: "Bolt", Icon: Bolt, keywords: ["闪电", "加速"] },
  { id: "atom", label: "Atom", Icon: Atom, keywords: ["科学", "研究"] },
  { id: "orbit", label: "Orbit", Icon: Orbit, keywords: ["太空", "系统"] },
  { id: "cpu", label: "CPU", Icon: Cpu, keywords: ["硬件", "算力"] },
  { id: "server", label: "Server", Icon: Server, keywords: ["服务器", "后端"] },
  { id: "terminal", label: "Terminal", Icon: Terminal, keywords: ["代码", "cli"] },
  { id: "code-2", label: "Code", Icon: Code2, keywords: ["编程", "开发"] },
  { id: "file-code-2", label: "FileCode", Icon: FileCode2, keywords: ["源码", "文件"] },
  { id: "bug", label: "Bug", Icon: Bug, keywords: ["调试", "修复"] },
  { id: "git-branch", label: "Git", Icon: GitBranch, keywords: ["版本", "分支"] },
  { id: "github", label: "GitHub", Icon: Github, keywords: ["开源", "仓库"] },
  { id: "database", label: "Database", Icon: Database, keywords: ["数据", "存储"] },
  { id: "layers", label: "Layers", Icon: Layers, keywords: ["架构", "分层"] },
  { id: "boxes", label: "Boxes", Icon: Boxes, keywords: ["模块", "组件"] },
  { id: "component", label: "Component", Icon: Component, keywords: ["组件", "UI"] },
  { id: "workflow", label: "Workflow", Icon: Workflow, keywords: ["流程", "编排"] },
  { id: "puzzle", label: "Puzzle", Icon: Puzzle, keywords: ["拼图", "扩展"] },
  { id: "plug", label: "Plug", Icon: Plug, keywords: ["插件", "接入"] },
  { id: "search", label: "Search", Icon: Search, keywords: ["检索", "搜索", "知识库"] },
  { id: "file-search", label: "Find", Icon: FileSearch, keywords: ["查找", "文档"] },
  { id: "scan-face", label: "Scan", Icon: ScanFace, keywords: ["识别", "扫描"] },
  { id: "fingerprint", label: "ID", Icon: Fingerprint, keywords: ["身份", "安全"] },
  { id: "eye", label: "Eye", Icon: Eye, keywords: ["观察", "预览"] },
  { id: "binoculars", label: "Scout", Icon: Binoculars, keywords: ["探索", "侦察"] },
  { id: "book-open", label: "Book", Icon: BookOpen, keywords: ["文档", "阅读"] },
  { id: "book-marked", label: "Marked", Icon: BookMarked, keywords: ["书签", "精读"] },
  { id: "bookmark", label: "Bookmark", Icon: Bookmark, keywords: ["收藏"] },
  { id: "library", label: "Library", Icon: Library, keywords: ["图书馆", "资料"] },
  { id: "file-text", label: "File", Icon: FileText, keywords: ["文稿", "写作"] },
  { id: "notebook-pen", label: "Notes", Icon: NotebookPen, keywords: ["笔记", "记录"] },
  { id: "clipboard-list", label: "List", Icon: ClipboardList, keywords: ["清单", "任务"] },
  { id: "list-todo", label: "Todo", Icon: ListTodo, keywords: ["待办", "计划"] },
  { id: "newspaper", label: "News", Icon: Newspaper, keywords: ["资讯", "媒体"] },
  { id: "quote", label: "Quote", Icon: Quote, keywords: ["引用", "文案"] },
  { id: "graduation-cap", label: "Learn", Icon: GraduationCap, keywords: ["教育", "学习"] },
  { id: "school", label: "School", Icon: School, keywords: ["学校", "课程"] },
  { id: "university", label: "Uni", Icon: University, keywords: ["高校", "学术"] },
  { id: "lightbulb", label: "Idea", Icon: Lightbulb, keywords: ["创意", "灵感"] },
  { id: "palette", label: "Palette", Icon: Palette, keywords: ["设计", "美术"] },
  { id: "paintbrush", label: "Brush", Icon: Paintbrush, keywords: ["绘画", "设计"] },
  { id: "brush", label: "Art", Icon: Brush, keywords: ["艺术", "创作"] },
  { id: "pen-line", label: "Pen", Icon: PenLine, keywords: ["写作", "编辑"] },
  { id: "pencil-ruler", label: "Draft", Icon: PencilRuler, keywords: ["制图", "设计"] },
  { id: "feather", label: "Feather", Icon: Feather, keywords: ["文案", "轻盈"] },
  { id: "image", label: "Image", Icon: Image, keywords: ["图片", "视觉"] },
  { id: "camera", label: "Camera", Icon: Camera, keywords: ["摄影", "拍摄"] },
  { id: "aperture", label: "Aperture", Icon: Aperture, keywords: ["镜头", "摄影"] },
  { id: "webcam", label: "Webcam", Icon: Webcam, keywords: ["直播", "视频"] },
  { id: "clapperboard", label: "Film", Icon: Clapperboard, keywords: ["视频", "影视"] },
  { id: "film", label: "Movie", Icon: Film, keywords: ["电影", "剪辑"] },
  { id: "video", label: "Video", Icon: Video, keywords: ["录像", "会议"] },
  { id: "tv", label: "TV", Icon: Tv, keywords: ["电视", "媒体"] },
  { id: "music", label: "Music", Icon: Music, keywords: ["音乐", "音频"] },
  { id: "headphones", label: "Audio", Icon: Headphones, keywords: ["播客", "听"] },
  { id: "mic", label: "Mic", Icon: Mic, keywords: ["语音", "采访"] },
  { id: "speaker", label: "Speaker", Icon: Speaker, keywords: ["扬声", "音响"] },
  { id: "radio", label: "Radio", Icon: Radio, keywords: ["广播", "信号"] },
  { id: "message-circle", label: "Chat", Icon: MessageCircle, keywords: ["对话", "客服"] },
  { id: "messages-square", label: "Forum", Icon: MessagesSquare, keywords: ["讨论", "社区"] },
  { id: "send", label: "Send", Icon: Send, keywords: ["发送", "消息"] },
  { id: "mail", label: "Mail", Icon: Mail, keywords: ["邮件", "通知"] },
  { id: "inbox", label: "Inbox", Icon: Inbox, keywords: ["收件", "消息"] },
  { id: "bell", label: "Bell", Icon: Bell, keywords: ["提醒", "通知"] },
  { id: "megaphone", label: "Announce", Icon: Megaphone, keywords: ["营销", "传播"] },
  { id: "share-2", label: "Share", Icon: Share2, keywords: ["分享", "社交"] },
  { id: "languages", label: "Translate", Icon: Languages, keywords: ["翻译", "语言"] },
  { id: "globe-2", label: "Globe", Icon: Globe2, keywords: ["国际", "网络"] },
  { id: "earth", label: "Earth", Icon: Earth, keywords: ["地球", "全球"] },
  { id: "map", label: "Map", Icon: Map, keywords: ["地图", "出行"] },
  { id: "map-pin", label: "Pin", Icon: MapPin, keywords: ["定位", "地点"] },
  { id: "navigation", label: "Nav", Icon: Navigation, keywords: ["导航", "方向"] },
  { id: "compass", label: "Compass", Icon: Compass, keywords: ["导航", "探索"] },
  { id: "route", label: "Route", Icon: Route, keywords: ["路线", "路径"] },
  { id: "milestone", label: "Mile", Icon: Milestone, keywords: ["里程碑", "进度"] },
  { id: "briefcase", label: "Work", Icon: Briefcase, keywords: ["商务", "职场"] },
  { id: "building-2", label: "Office", Icon: Building2, keywords: ["公司", "办公"] },
  { id: "store", label: "Store", Icon: Store, keywords: ["商店", "零售"] },
  { id: "shopping-bag", label: "Shop", Icon: ShoppingBag, keywords: ["购物", "电商"] },
  { id: "warehouse", label: "Warehouse", Icon: Warehouse, keywords: ["仓储", "库存"] },
  { id: "factory", label: "Factory", Icon: Factory, keywords: ["工厂", "制造"] },
  { id: "folder-kanban", label: "Project", Icon: FolderKanban, keywords: ["项目", "看板"] },
  { id: "layout-dashboard", label: "Board", Icon: LayoutDashboard, keywords: ["仪表盘", "后台"] },
  { id: "presentation", label: "Slides", Icon: Presentation, keywords: ["演示", "汇报"] },
  { id: "users", label: "Team", Icon: Users, keywords: ["团队", "协作"] },
  { id: "user-round", label: "User", Icon: UserRound, keywords: ["用户", "个人"] },
  { id: "contact", label: "Contact", Icon: Contact, keywords: ["联系人", "名片"] },
  { id: "heart-handshake", label: "Care", Icon: HeartHandshake, keywords: ["合作", "关怀"] },
  { id: "hand-heart", label: "Help", Icon: HandHeart, keywords: ["帮助", "支持"] },
  { id: "target", label: "Target", Icon: Target, keywords: ["目标", "专注"] },
  { id: "trophy", label: "Trophy", Icon: Trophy, keywords: ["成就", "竞赛"] },
  { id: "medal", label: "Medal", Icon: Medal, keywords: ["奖牌", "荣誉"] },
  { id: "award", label: "Award", Icon: Award, keywords: ["奖项", "认证"] },
  { id: "crown", label: "Crown", Icon: Crown, keywords: ["顶级", "VIP"] },
  { id: "badge-check", label: "Verified", Icon: BadgeCheck, keywords: ["认证", "通过"] },
  { id: "chart-column", label: "Chart", Icon: ChartColumn, keywords: ["数据", "分析"] },
  { id: "bar-chart-3", label: "Bars", Icon: BarChart3, keywords: ["柱状图", "统计"] },
  { id: "chart-pie", label: "Pie", Icon: ChartPie, keywords: ["饼图", "占比"] },
  { id: "pie-chart", label: "Ratio", Icon: PieChart, keywords: ["比例", "分析"] },
  { id: "table-2", label: "Table", Icon: Table2, keywords: ["表格", "数据"] },
  { id: "calculator", label: "Calc", Icon: Calculator, keywords: ["计算", "财务"] },
  { id: "banknote", label: "Cash", Icon: Banknote, keywords: ["金钱", "财务"] },
  { id: "wallet", label: "Wallet", Icon: Wallet, keywords: ["钱包", "支付"] },
  { id: "credit-card", label: "Card", Icon: CreditCard, keywords: ["信用卡", "支付"] },
  { id: "piggy-bank", label: "Save", Icon: PiggyBank, keywords: ["储蓄", "理财"] },
  { id: "landmark", label: "Bank", Icon: Landmark, keywords: ["银行", "金融"] },
  { id: "scale", label: "Legal", Icon: Scale, keywords: ["法律", "合规"] },
  { id: "shield", label: "Shield", Icon: Shield, keywords: ["安全", "防护"] },
  { id: "lock", label: "Lock", Icon: Lock, keywords: ["加密", "隐私"] },
  { id: "key-round", label: "Key", Icon: KeyRound, keywords: ["密钥", "权限"] },
  { id: "stethoscope", label: "Health", Icon: Stethoscope, keywords: ["医疗", "健康"] },
  { id: "microscope", label: "Micro", Icon: Microscope, keywords: ["显微", "科研"] },
  { id: "flask-conical", label: "Lab", Icon: FlaskConical, keywords: ["实验", "科研"] },
  { id: "thermometer", label: "Temp", Icon: Thermometer, keywords: ["温度", "监测"] },
  { id: "dumbbell", label: "Fitness", Icon: Dumbbell, keywords: ["健身", "运动"] },
  { id: "volleyball", label: "Sport", Icon: Volleyball, keywords: ["运动", "球类"] },
  { id: "bike", label: "Bike", Icon: Bike, keywords: ["骑行", "出行"] },
  { id: "hammer", label: "Build", Icon: Hammer, keywords: ["建造", "工具"] },
  { id: "wrench", label: "Fix", Icon: Wrench, keywords: ["维修", "运维"] },
  { id: "cog", label: "Cog", Icon: Cog, keywords: ["设置", "机械"] },
  { id: "settings-2", label: "Settings", Icon: Settings2, keywords: ["配置", "偏好"] },
  { id: "pocket-knife", label: "Tool", Icon: PocketKnife, keywords: ["工具", "实用"] },
  { id: "scissors", label: "Cut", Icon: Scissors, keywords: ["剪切", "编辑"] },
  { id: "printer", label: "Print", Icon: Printer, keywords: ["打印", "输出"] },
  { id: "paperclip", label: "Attach", Icon: Paperclip, keywords: ["附件", "链接"] },
  { id: "link-2", label: "Link", Icon: Link2, keywords: ["链接", "关联"] },
  { id: "package", label: "Package", Icon: Package, keywords: ["包裹", "发布"] },
  { id: "box", label: "Box", Icon: Box, keywords: ["盒子", "模块"] },
  { id: "archive", label: "Archive", Icon: Archive, keywords: ["归档", "存储"] },
  { id: "gamepad-2", label: "Game", Icon: Gamepad2, keywords: ["游戏", "娱乐"] },
  { id: "ghost", label: "Ghost", Icon: Ghost, keywords: ["趣味", "幽灵"] },
  { id: "drama", label: "Drama", Icon: Drama, keywords: ["戏剧", "表演"] },
  { id: "party-popper", label: "Party", Icon: PartyPopper, keywords: ["庆祝", "活动"] },
  { id: "gift", label: "Gift", Icon: Gift, keywords: ["礼物", "福利"] },
  { id: "gem", label: "Gem", Icon: Gem, keywords: ["精品", "价值"] },
  { id: "star", label: "Star", Icon: Star, keywords: ["收藏", "明星"] },
  { id: "heart", label: "Heart", Icon: Heart, keywords: ["关怀", "生活"] },
  { id: "smile", label: "Smile", Icon: Smile, keywords: ["开心", "表情"] },
  { id: "laugh", label: "Laugh", Icon: Laugh, keywords: ["幽默", "开心"] },
  { id: "skull", label: "Skull", Icon: Skull, keywords: ["警示", "危险"] },
  { id: "moon", label: "Moon", Icon: Moon, keywords: ["夜间", "冷静"] },
  { id: "sun", label: "Sun", Icon: Sun, keywords: ["白天", "阳光"] },
  { id: "sunrise", label: "Sunrise", Icon: Sunrise, keywords: ["清晨", "开始"] },
  { id: "cloud", label: "Cloud", Icon: Cloud, keywords: ["云", "天气"] },
  { id: "cloud-sun", label: "Weather", Icon: CloudSun, keywords: ["天气", "晴"] },
  { id: "rainbow", label: "Rainbow", Icon: Rainbow, keywords: ["彩虹", "多彩"] },
  { id: "snowflake", label: "Snow", Icon: Snowflake, keywords: ["雪花", "冷却"] },
  { id: "wind", label: "Wind", Icon: Wind, keywords: ["风", "气流"] },
  { id: "waves", label: "Waves", Icon: Waves, keywords: ["海浪", "流动"] },
  { id: "droplets", label: "Water", Icon: Droplets, keywords: ["水", "液体"] },
  { id: "flame", label: "Flame", Icon: Flame, keywords: ["火焰", "热"] },
  { id: "trees", label: "Nature", Icon: Trees, keywords: ["自然", "环保"] },
  { id: "leaf", label: "Leaf", Icon: Leaf, keywords: ["叶子", "绿色"] },
  { id: "sprout", label: "Sprout", Icon: Sprout, keywords: ["成长", "萌芽"] },
  { id: "flower-2", label: "Flower", Icon: Flower2, keywords: ["花朵", "美丽"] },
  { id: "mountain", label: "Mountain", Icon: Mountain, keywords: ["山", "户外"] },
  { id: "tent", label: "Camp", Icon: Tent, keywords: ["露营", "户外"] },
  { id: "recycle", label: "Eco", Icon: Recycle, keywords: ["回收", "环保"] },
  { id: "bird", label: "Bird", Icon: Bird, keywords: ["鸟", "自由"] },
  { id: "cat", label: "Cat", Icon: Cat, keywords: ["猫", "宠物"] },
  { id: "dog", label: "Dog", Icon: Dog, keywords: ["狗", "宠物"] },
  { id: "fish", label: "Fish", Icon: Fish, keywords: ["鱼", "海洋"] },
  { id: "shell", label: "Shell", Icon: Shell, keywords: ["贝壳", "海边"] },
  { id: "home", label: "Home", Icon: Home, keywords: ["家", "主页"] },
  { id: "sofa", label: "Living", Icon: Sofa, keywords: ["家居", "生活"] },
  { id: "coffee", label: "Coffee", Icon: Coffee, keywords: ["咖啡", "休息"] },
  { id: "utensils", label: "Food", Icon: Utensils, keywords: ["餐饮", "美食"] },
  { id: "cookie", label: "Cookie", Icon: Cookie, keywords: ["甜品", "零食"] },
  { id: "apple", label: "Apple", Icon: Apple, keywords: ["水果", "健康"] },
  { id: "grape", label: "Grape", Icon: Grape, keywords: ["葡萄", "水果"] },
  { id: "egg", label: "Egg", Icon: Egg, keywords: ["鸡蛋", "早餐"] },
  { id: "wine", label: "Wine", Icon: Wine, keywords: ["酒", "餐饮"] },
  { id: "plane", label: "Plane", Icon: Plane, keywords: ["飞行", "旅行"] },
  { id: "car", label: "Car", Icon: Car, keywords: ["汽车", "出行"] },
  { id: "bus", label: "Bus", Icon: Bus, keywords: ["公交", "出行"] },
  { id: "train-front", label: "Train", Icon: TrainFront, keywords: ["火车", "出行"] },
  { id: "truck", label: "Truck", Icon: Truck, keywords: ["货运", "物流"] },
  { id: "ship", label: "Ship", Icon: Ship, keywords: ["航运", "物流"] },
  { id: "sailboat", label: "Sail", Icon: Sailboat, keywords: ["帆船", "航海"] },
  { id: "anchor", label: "Anchor", Icon: Anchor, keywords: ["锚定", "稳定"] },
  { id: "flag", label: "Flag", Icon: Flag, keywords: ["旗帜", "标记"] },
  { id: "sword", label: "Sword", Icon: Sword, keywords: ["战斗", "挑战"] },
  { id: "gauge", label: "Gauge", Icon: Gauge, keywords: ["仪表", "性能"] },
  { id: "timer", label: "Timer", Icon: Timer, keywords: ["计时", "倒计时"] },
  { id: "clock", label: "Clock", Icon: Clock, keywords: ["时间", "时钟"] },
  { id: "alarm-clock", label: "Alarm", Icon: AlarmClock, keywords: ["闹钟", "提醒"] },
  { id: "hourglass", label: "Wait", Icon: Hourglass, keywords: ["等待", "沙漏"] },
  { id: "calendar", label: "Calendar", Icon: Calendar, keywords: ["日历", "日程"] },
  { id: "watch", label: "Watch", Icon: Watch, keywords: ["手表", "时间"] },
  { id: "laptop", label: "Laptop", Icon: Laptop, keywords: ["笔记本", "办公"] },
  { id: "monitor-smartphone", label: "Devices", Icon: MonitorSmartphone, keywords: ["多端", "设备"] },
  { id: "tablet-smartphone", label: "Mobile", Icon: TabletSmartphone, keywords: ["移动", "手机"] },
  { id: "phone", label: "Phone", Icon: Phone, keywords: ["电话", "通话"] },
  { id: "bluetooth", label: "BT", Icon: Bluetooth, keywords: ["蓝牙", "连接"] },
  { id: "wifi", label: "WiFi", Icon: Wifi, keywords: ["网络", "无线"] },
  { id: "signal", label: "Signal", Icon: Signal, keywords: ["信号", "强度"] },
  { id: "battery-charging", label: "Power", Icon: BatteryCharging, keywords: ["电量", "充电"] },
  { id: "umbrella", label: "Umbrella", Icon: Umbrella, keywords: ["保护", "雨伞"] },
  { id: "glasses", label: "Glasses", Icon: Glasses, keywords: ["眼镜", "阅读"] },
  { id: "ear", label: "Ear", Icon: Ear, keywords: ["倾听", "听力"] },
  { id: "footprints", label: "Steps", Icon: Footprints, keywords: ["足迹", "步骤"] },
  { id: "mouse-pointer-2", label: "Click", Icon: MousePointer2, keywords: ["点击", "指针"] },
  { id: "shuffle", label: "Shuffle", Icon: Shuffle, keywords: ["随机", "打乱"] },
  { id: "check-circle-2", label: "Done", Icon: CheckCircle2, keywords: ["完成", "成功"] },
  { id: "trash-2", label: "Trash", Icon: Trash2, keywords: ["删除", "清理"] },
  { id: "fire-extinguisher", label: "Safety", Icon: FireExtinguisher, keywords: ["安全", "应急"] },
  // Loop 节点 & App 图标
  { id: "hand", label: "Hand", Icon: Hand, keywords: ["手动", "触发", "manual"] },
  { id: "webhook", label: "Webhook", Icon: Webhook, keywords: ["回调", "钩子", "API"] },
  { id: "tag", label: "Tag", Icon: Tag, keywords: ["标签", "分类"] },
  { id: "captions", label: "Captions", Icon: Captions, keywords: ["字幕", "文字"] },
  { id: "audio-lines", label: "Audio", Icon: AudioLines, keywords: ["音频", "语音", "TTS"] },
  { id: "audio-waveform", label: "Waveform", Icon: AudioWaveform, keywords: ["波形", "音频处理"] },
  { id: "git-fork", label: "Fork", Icon: GitFork, keywords: ["分支", "分叉"] },
  { id: "git-merge", label: "Merge", Icon: GitMerge, keywords: ["合并", "聚合"] },
  { id: "filter", label: "Filter", Icon: Filter, keywords: ["过滤", "筛选"] },
  { id: "repeat", label: "Repeat", Icon: Repeat, keywords: ["循环", "重复", "loop"] },
  { id: "user-check", label: "Approve", Icon: UserCheck, keywords: ["审批", "确认", "人工"] },
  { id: "braces", label: "Braces", Icon: Braces, keywords: ["JSON", "字段", "数据"] },
  { id: "type", label: "Type", Icon: Type, keywords: ["文字", "格式化", "字体"] },
  { id: "file-json", label: "JSON", Icon: FileJson, keywords: ["JSON", "文件"] },
  { id: "code", label: "Code", Icon: Code, keywords: ["代码", "编程"] },
  { id: "arrow-up-down", label: "Sort", Icon: ArrowUpDown, keywords: ["排序", "上下"] },
  { id: "sigma", label: "Sigma", Icon: Sigma, keywords: ["聚合", "求和", "数学"] },
  { id: "globe", label: "Globe", Icon: Globe, keywords: ["网络", "HTTP", "全球"] },
  { id: "play", label: "Play", Icon: Play, keywords: ["运行", "播放", "执行"] },
  { id: "arrow-right-from-line", label: "Output", Icon: ArrowRightFromLine, keywords: ["输出", "导出"] },
  { id: "mouse-pointer-click", label: "Click", Icon: MousePointerClick, keywords: ["点击", "触发"] },
];

export const LUCIDE_AGENT_ICONS: LucideAgentIcon[] =
  LUCIDE_AGENT_ICON_COMPONENTS.flatMap((item) => {
    const name = item.Icon.displayName;
    const data = name ? LUCIDE_DATA_BY_NAME[name] : undefined;
    return data ? [{ ...item, data }] : [];
  });

export function filterLucideAgentIcons(query: string): LucideAgentIcon[] {
  const q = query.trim().toLowerCase();
  if (!q) return LUCIDE_AGENT_ICONS;
  return LUCIDE_AGENT_ICONS.filter((item) => {
    const hay = [item.id, item.label, ...item.keywords].join(" ").toLowerCase();
    return hay.includes(q);
  });
}

function hashSeed(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/**
 * 按名称 / 职能文本自动挑选 Lucide 图标（对齐后端 memory::auto_icon）。
 * 无命中时用 name 哈希稳定选取，避免总是同一种。
 */
export function suggestLucideAgentIcon(
  name: string,
  extras: string[] = [],
): LucideAgentIcon {
  const hay = [name, ...extras].join(" ").toLowerCase().trim();
  let best = LUCIDE_AGENT_ICONS[0];
  let bestScore = 0;
  const tied: LucideAgentIcon[] = [];

  for (const item of LUCIDE_AGENT_ICONS) {
    let score = 0;
    if (hay.includes(item.id)) score += 4;
    for (const kw of item.keywords) {
      const k = kw.toLowerCase();
      if (hay.includes(k)) score += 1 + Math.floor([...k].length / 2);
    }
    if (item.id === "bot" || item.id === "sparkles") {
      score = Math.max(0, score - 1);
    }
    if (score > bestScore) {
      bestScore = score;
      best = item;
      tied.length = 0;
      tied.push(item);
    } else if (score === bestScore && score > 0) {
      tied.push(item);
    }
  }

  if (bestScore === 0) {
    const idx = hashSeed(name.trim() || "agent") % LUCIDE_AGENT_ICONS.length;
    return LUCIDE_AGENT_ICONS[idx]!;
  }
  if (tied.length <= 1) return best;
  return tied[hashSeed(name.trim() || "agent") % tied.length]!;
}

/** 按 Lucide kebab-case id 解析图标组件（工具 catalog 的 emoji 字段） */
export function resolveLucideIconById(id: string | null | undefined): LucideIconComponent | null {
  if (!id) return null;
  const key = id.trim().toLowerCase();
  if (!key) return null;
  return LUCIDE_AGENT_ICONS.find((item) => item.id === key)?.Icon ?? null;
}

/** 浏览器端将 Lucide 图标渲染为 SVG base64（写入 assets/emoji.svg） */
export async function lucideIconToSvgBase64Async(
  Icon: LucideIconComponent,
  options?: {
    paint?: LucidePaint;
    color?: string;
    size?: number;
    strokeWidth?: number;
    style?: LucideRenderStyle;
  },
): Promise<string> {
  const host = document.createElement("div");
  host.setAttribute("aria-hidden", "true");
  host.style.cssText = "position:fixed;left:-99999px;top:0;width:0;height:0;overflow:hidden;";
  document.body.appendChild(host);
  const root = createRoot(host);
  const paint: LucidePaint =
    options?.paint ??
    solidPaint(options?.color ?? DEFAULT_LUCIDE_ICON_COLOR);
  const style = options?.style ?? DEFAULT_LUCIDE_RENDER_STYLE;
  try {
    root.render(
      createElement(Icon, {
        size: options?.size ?? 128,
        color: paint.kind === "solid" ? paint.color : "#0f172a",
        strokeWidth: options?.strokeWidth ?? 2,
      }),
    );
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    const svg = host.querySelector("svg");
    if (!svg) throw new Error("Lucide SVG render failed");
    svg.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    applyPaintToSvg(svg, paint, style);
    const xml = new XMLSerializer().serializeToString(svg);
    return btoa(unescape(encodeURIComponent(xml)));
  } finally {
    root.unmount();
    host.remove();
  }
}

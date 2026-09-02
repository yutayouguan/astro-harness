export interface VoiceOption {
  value: string;
  label: string;
  group: string;
}

export const MINIMAX_VOICES: VoiceOption[] = [
  // ── 中文 · 通用 ──
  { value: "male-qn-qingse", label: "青涩青年", group: "中文 · 通用" },
  { value: "male-qn-jingying", label: "精英青年", group: "中文 · 通用" },
  { value: "male-qn-badao", label: "霸道青年", group: "中文 · 通用" },
  { value: "male-qn-daxuesheng", label: "大学生", group: "中文 · 通用" },
  { value: "female-shaonv", label: "少女", group: "中文 · 通用" },
  { value: "female-yujie", label: "御姐", group: "中文 · 通用" },
  { value: "female-chengshu", label: "成熟女性", group: "中文 · 通用" },
  { value: "female-tianmei", label: "甜美女性", group: "中文 · 通用" },
  // ── 中文 · 精品 ──
  {
    value: "male-qn-qingse-jingpin",
    label: "青涩青年 β",
    group: "中文 · 精品",
  },
  {
    value: "male-qn-jingying-jingpin",
    label: "精英青年 β",
    group: "中文 · 精品",
  },
  { value: "male-qn-badao-jingpin", label: "霸道青年 β", group: "中文 · 精品" },
  {
    value: "male-qn-daxuesheng-jingpin",
    label: "大学生 β",
    group: "中文 · 精品",
  },
  { value: "female-shaonv-jingpin", label: "少女 β", group: "中文 · 精品" },
  { value: "female-yujie-jingpin", label: "御姐 β", group: "中文 · 精品" },
  {
    value: "female-chengshu-jingpin",
    label: "成熟女性 β",
    group: "中文 · 精品",
  },
  {
    value: "female-tianmei-jingpin",
    label: "甜美女性 β",
    group: "中文 · 精品",
  },
  // ── 中文 · 角色 ──
  { value: "bingjiao_didi", label: "病娇弟弟", group: "中文 · 角色" },
  { value: "junlang_nanyou", label: "俊朗男友", group: "中文 · 角色" },
  { value: "chunzhen_xuedi", label: "纯真学弟", group: "中文 · 角色" },
  { value: "lengdan_xiongzhang", label: "冷淡学长", group: "中文 · 角色" },
  { value: "badao_shaoye", label: "霸道少爷", group: "中文 · 角色" },
  { value: "tianxin_xiaoling", label: "甜心小玲", group: "中文 · 角色" },
  { value: "qiaopi_mengmei", label: "俏皮萌妹", group: "中文 · 角色" },
  { value: "wumei_yujie", label: "妩媚御姐", group: "中文 · 角色" },
  { value: "diadia_xuemei", label: "嗲嗲学妹", group: "中文 · 角色" },
  { value: "danya_xuejie", label: "淡雅学姐", group: "中文 · 角色" },
  // ── 中文 · 儿童 ──
  { value: "clever_boy", label: "聪明男童", group: "中文 · 儿童" },
  { value: "cute_boy", label: "可爱男童", group: "中文 · 儿童" },
  { value: "lovely_girl", label: "萌萌女童", group: "中文 · 儿童" },
  { value: "cartoon_pig", label: "卡通猪小琪", group: "中文 · 儿童" },
  // ── 中文 · 播报 ──
  {
    value: "Chinese (Mandarin)_Reliable_Executive",
    label: "沉稳高管",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_News_Anchor",
    label: "新闻女声",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Male_Announcer",
    label: "播报男声",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Radio_Host",
    label: "电台男主播",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Lyrical_Voice",
    label: "抒情男声",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Gentleman",
    label: "温润男声",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Sweet_Lady",
    label: "甜美女声",
    group: "中文 · 播报",
  },
  {
    value: "Chinese (Mandarin)_Warm_Bestie",
    label: "温暖闺蜜",
    group: "中文 · 播报",
  },
  // ── 中文 · 特色 ──
  {
    value: "Chinese (Mandarin)_Mature_Woman",
    label: "傲娇御姐",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Unrestrained_Young_Man",
    label: "不羁青年",
    group: "中文 · 特色",
  },
  { value: "Arrogant_Miss", label: "嚣张小姐", group: "中文 · 特色" },
  { value: "Robot_Armor", label: "机械战甲", group: "中文 · 特色" },
  {
    value: "Chinese (Mandarin)_Kind-hearted_Antie",
    label: "热心大婶",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_HK_Flight_Attendant",
    label: "港普空姐",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Humorous_Elder",
    label: "搞笑大爷",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Kind-hearted_Elder",
    label: "花甲奶奶",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Cute_Spirit",
    label: "憨憨萌兽",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Southern_Young_Man",
    label: "南方小哥",
    group: "中文 · 特色",
  },
  {
    value: "Chinese (Mandarin)_Stubborn_Friend",
    label: "嘴硬竹马",
    group: "中文 · 特色",
  },
  // ── 中文 · 温柔 ──
  {
    value: "Chinese (Mandarin)_Gentle_Youth",
    label: "温润青年",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Warm_Girl",
    label: "温暖少女",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Gentle_Senior",
    label: "温柔学姐",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Crisp_Girl",
    label: "清脆少女",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Soft_Girl",
    label: "柔和少女",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Pure-hearted_Boy",
    label: "清澈邻家弟弟",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Sincere_Adult",
    label: "真诚青年",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Straightforward_Boy",
    label: "率真弟弟",
    group: "中文 · 温柔",
  },
  {
    value: "Chinese (Mandarin)_Wise_Women",
    label: "阅历姐姐",
    group: "中文 · 温柔",
  },
  // ── 粤语 ──
  {
    value: "Cantonese_ProfessionalHost（F)",
    label: "专业女主持",
    group: "粤语",
  },
  { value: "Cantonese_GentleLady", label: "温柔女声", group: "粤语" },
  {
    value: "Cantonese_ProfessionalHost（M)",
    label: "专业男主持",
    group: "粤语",
  },
  { value: "Cantonese_PlayfulMan", label: "活泼男声", group: "粤语" },
  { value: "Cantonese_CuteGirl", label: "可爱女孩", group: "粤语" },
  { value: "Cantonese_KindWoman", label: "善良女声", group: "粤语" },
  // ── English ──
  {
    value: "English_Trustworthy_Man",
    label: "Trustworthy Man",
    group: "English",
  },
  { value: "English_Graceful_Lady", label: "Graceful Lady", group: "English" },
  { value: "English_Aussie_Bloke", label: "Aussie Bloke", group: "English" },
  {
    value: "English_Whispering_girl",
    label: "Whispering Girl",
    group: "English",
  },
  { value: "English_Diligent_Man", label: "Diligent Man", group: "English" },
  {
    value: "English_Gentle-voiced_man",
    label: "Gentle-voiced Man",
    group: "English",
  },
  { value: "Sweet_Girl", label: "Sweet Girl", group: "English" },
  { value: "Attractive_Girl", label: "Attractive Girl", group: "English" },
  { value: "Serene_Woman", label: "Serene Woman", group: "English" },
  { value: "Charming_Lady", label: "Charming Lady", group: "English" },
  // ── 日本語 ──
  { value: "Japanese_GentleButler", label: "Gentle Butler", group: "日本語" },
  { value: "Japanese_KindLady", label: "Kind Lady", group: "日本語" },
  { value: "Japanese_CalmLady", label: "Calm Lady", group: "日本語" },
  {
    value: "Japanese_OptimisticYouth",
    label: "Optimistic Youth",
    group: "日本語",
  },
  {
    value: "Japanese_DecisivePrincess",
    label: "Decisive Princess",
    group: "日本語",
  },
  { value: "Japanese_DominantMan", label: "Dominant Man", group: "日本語" },
  // ── 한국어 ──
  { value: "Korean_SweetGirl", label: "Sweet Girl", group: "한국어" },
  {
    value: "Korean_CheerfulBoyfriend",
    label: "Cheerful Boyfriend",
    group: "한국어",
  },
  {
    value: "Korean_EnchantingSister",
    label: "Enchanting Sister",
    group: "한국어",
  },
  { value: "Korean_CalmGentleman", label: "Calm Gentleman", group: "한국어" },
];

export const OPENAI_VOICES: VoiceOption[] = [
  { value: "alloy", label: "Alloy", group: "OpenAI" },
  { value: "echo", label: "Echo", group: "OpenAI" },
  { value: "fable", label: "Fable", group: "OpenAI" },
  { value: "onyx", label: "Onyx", group: "OpenAI" },
  { value: "nova", label: "Nova", group: "OpenAI" },
  { value: "shimmer", label: "Shimmer", group: "OpenAI" },
];

export function getVoiceOptions(providerId: string): VoiceOption[] {
  if (providerId === "minimax") return MINIMAX_VOICES;
  if (providerId === "openai") return OPENAI_VOICES;
  if (!providerId) return [];
  return [];
}

export function getVoiceGroups(voices: VoiceOption[]): string[] {
  const seen = new Set<string>();
  return voices
    .filter((v) => {
      if (seen.has(v.group)) return false;
      seen.add(v.group);
      return true;
    })
    .map((v) => v.group);
}

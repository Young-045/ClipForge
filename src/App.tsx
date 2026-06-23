import { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { DEFAULT_SHORTCUT, SHORTCUT_STORAGE_KEY } from "./shortcuts";
import "./App.css";

const PAGE_SIZE = 20;
const SEARCH_DEBOUNCE_MS = 300;

type ClipboardItem = {
  id: number;
  content: string;
  content_type: string;
  image_path: string;
  html_content: string;
  created_at: string;
  group_id?: number;
  group_name?: string;
};

type CustomGroup = {
  id: number;
  name: string;
  created_at: string;
  color?: string;
};

type FilterMode =
  | { kind: "all" }
  | { kind: "type"; content_type: string }
  | { kind: "group"; group_id: number; group_name: string };

const TYPE_GROUPS = [
  { key: "text", label: "文本" },
  { key: "image", label: "图片" },
  { key: "code", label: "代码" },
  { key: "url", label: "链接" },
  { key: "email", label: "邮箱" },
] as const;

// content_type -> display label
const TYPE_LABEL: Record<string, string> = {
  text: "文本",
  url: "链接",
  email: "邮箱",
  code: "代码",
  color: "颜色",
  image: "图片",
  path: "路径",
};

// ── App ─────────────────────────────────────────────────────────────
function isLightColor(hex: string): boolean {
  if (!hex || hex.length < 7) return false;
  const r = parseInt(hex.substring(1, 3), 16);
  const g = parseInt(hex.substring(3, 5), 16);
  const b = parseInt(hex.substring(5, 7), 16);
  // relative luminance (sRGB)
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6;
}

function App() {
  const [page, setPage] = useState<"main" | "settings">("main");

  function goToPage(p: "main" | "settings") {
    setPage(p);
    invoke("set_current_page", { page: p }).catch(() => {});
  }

  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(true);
  const [search, setSearch] = useState("");
  const [popup, setPopup] = useState("");
  const [popupError, setPopupError] = useState(false);
  const popupTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [imageCache, setImageCache] = useState<Record<string, string>>({});

  // Filter: tab-based grouping
  const [filter, setFilter] = useState<FilterMode>({ kind: "all" });
  const [customGroups, setCustomGroups] = useState<CustomGroup[]>([]);
  const [refreshSig, setRefreshSig] = useState(0);

  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const listScrollRef = useRef<HTMLUListElement>(null);
  const loadingRef = useRef(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);

  // Decide which Rust command to call based on filter
  function resolveFilterInvoke() {
    if (filter.kind === "type") {
      return { cmd: "list_items_by_type" as const, args: { contentType: filter.content_type } };
    }
    if (filter.kind === "group") {
      return { cmd: "list_items_by_group" as const, args: { groupId: filter.group_id } };
    }
    return { cmd: "list_recent_items" as const, args: {} as Record<string, never> };
  }

  // ── Load paginated items ────────────────────────────────────────
  const loadItems = useCallback(
    async (reset = false) => {
      if (loadingRef.current) return;
      try {
        loadingRef.current = true;
        setLoadingMore(true);
        const o = reset ? 0 : offset;
        const { cmd, args } = resolveFilterInvoke();
        const fullArgs = { ...args, offset: o, limit: PAGE_SIZE };
        const result = await invoke<ClipboardItem[]>(cmd, fullArgs);
        if (reset) {
          setItems(result);
          setOffset(result.length);
          setHasMore(result.length >= PAGE_SIZE);
        } else {
          setItems((prev) => [...prev, ...result]);
          setOffset(o + result.length);
          setHasMore(result.length >= PAGE_SIZE);
        }
        for (const item of result) {
          if (item.content_type === "image" && item.image_path) {
            setImageCache((prev) => {
              if (prev[item.image_path]) return prev;
              invoke<string>("get_image_base64", { imagePath: item.image_path })
                .then((dataUrl) => setImageCache((p) => ({ ...p, [item.image_path]: dataUrl })))
                .catch(() => {});
              return prev;
            });
          }
        }
      } catch (e) {
        console.error("Failed to load items:", e);
      } finally {
        loadingRef.current = false;
        setLoadingMore(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [offset, filter, refreshSig]
  );

  // Load custom groups
  const loadGroups = useCallback(async () => {
    try {
      const gs = await invoke<CustomGroup[]>("list_groups");
      setCustomGroups(gs);
    } catch (e) {
      console.error("Failed to load groups:", e);
    }
  }, []);

  // ── Server-side search ───────────────────────────────────────────
  const doSearch = useCallback(
    async (keyword: string) => {
      const trimmed = keyword.trim();
      if (!trimmed) {
        setOffset(0);
        setHasMore(true);
        try {
          const { cmd, args } = resolveFilterInvoke();
          const result = await invoke<ClipboardItem[]>(cmd, { ...args, offset: 0, limit: PAGE_SIZE });
          setItems(result);
          setOffset(result.length);
          setHasMore(result.length >= PAGE_SIZE);
        } catch (e) {
          console.error("Failed to reload items:", e);
        }
        return;
      }
      try {
        const result = await invoke<ClipboardItem[]>("search_items", { keyword: trimmed });
        setItems(result);
        setHasMore(false);
      } catch (e) {
        console.error("Search failed:", e);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [filter]
  );

  // Debounced search
  useEffect(() => {
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => void doSearch(search), SEARCH_DEBOUNCE_MS);
    return () => { if (searchTimer.current) clearTimeout(searchTimer.current); };
  }, [search, doSearch]);

  // Initial load + clipboard updates
  useEffect(() => {
    loadItems(true);
    const setup = listen("clipboard-update", () => loadItems(true));
    return () => { setup.then((fn) => fn()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filter, refreshSig]);

  // Load groups on startup
  useEffect(() => { loadGroups(); }, [refreshSig, loadGroups]);

  // Infinite scroll
  useEffect(() => {
    const el = listScrollRef.current;
    if (!el) return;
    const handleScroll = () => {
      const threshold = 120;
      if (el.scrollHeight - el.scrollTop - el.clientHeight < threshold && hasMore && !loadingMore && !search) {
        loadItems(false);
      }
    };
    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => el.removeEventListener("scroll", handleScroll);
  }, [hasMore, loadingMore, search, loadItems]);

  // Scroll to top when filter changes
  useEffect(() => {
    listScrollRef.current?.scrollTo({ top: 0, behavior: "instant" as ScrollBehavior });
  }, [filter]);

  // Disable native context menu
  useEffect(() => {
    function preventContextMenu(e: MouseEvent) { e.preventDefault(); }
    document.addEventListener("contextmenu", preventContextMenu);
    return () => document.removeEventListener("contextmenu", preventContextMenu);
  }, []);

  // auto-hide
  useEffect(() => {
    const setup = listen("auto-hide", () => {
      void invoke("hide_window").catch(() => getCurrentWindow().hide().catch(() => {}));
    });
    return () => { setup.then((fn) => fn()); };
  }, []);

  // ── Item actions ──────────────────────────────────────────────────
  async function copyAndPaste(content: string) {
    if (!content) return;
    try {
      await invoke("copy_and_paste", { content });
    } catch (e) {
      console.error("copy_and_paste failed:", e);
      showPopup("复制失败", true);
    }
  }

  async function copyAndPasteImage(imagePath: string) {
    if (!imagePath) return;
    try {
      await invoke("copy_and_paste_image", { imagePath });
    } catch (e) {
      console.error("copy_and_paste_image failed:", e);
      showPopup("复制失败", true);
    }
  }

  async function deleteItem(id: number) {
    await invoke("delete_item", { id });
    setItems((prev) => prev.filter((item) => item.id !== id));
    showPopup("已删除");
  }

  async function clearAll() {
    await invoke("clear_all_items");
    setItems([]);
    setOffset(0);
    setHasMore(false);
    setImageCache({});
    showPopup("已清空全部记录");
  }

  async function moveToGroup(itemId: number, groupId: number | null, groupName?: string | null) {
    try {
      await invoke("set_item_group", { itemId, groupId });
      if (filter.kind === "group" && groupId === null) {
        setItems((prev) => prev.filter((it) => it.id !== itemId));
      } else {
        setItems((prev) =>
          prev.map((it) =>
            it.id === itemId ? { ...it, group_id: groupId ?? undefined, group_name: groupName ?? undefined } : it
          )
        );
      }
      showPopup(groupId ? "已加入分组" : "已移出分组");
    } catch (e) {
      console.error("set_item_group failed:", e);
      showPopup("分组操作失败", true);
    }
  }

  function showPopup(msg: string, isError = false) {
    setPopup(msg);
    setPopupError(isError);
    if (popupTimer.current) clearTimeout(popupTimer.current);
    popupTimer.current = setTimeout(() => setPopup(""), 2500);
  }

  async function hideApp() {
    await invoke("hide_window").catch(() => getCurrentWindow().hide().catch(() => {}));
  }

  // ── Titlebar ──────────────────────────────────────────────────────
  function Titlebar() {
    const isSettings = page === "settings";
    return (
      <div className="titlebar" data-tauri-drag-region onContextMenu={(e) => e.preventDefault()}>
        <div className="titlebar-left">
          <div className="app-icon">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <rect x="4" y="2" width="8" height="12" rx="1.5" />
              <path d="M6 2v1a1 1 0 0 0 1 1h2a1 1 0 0 0 1-1V2" />
              <path d="M6 7h4M6 10h3" />
            </svg>
          </div>
          <span className="app-name">ClipForge</span>
          {isSettings ? (
            <span className="app-badge">设置</span>
          ) : (
            <span className="app-badge">{items.length} 条</span>
          )}
        </div>
        <div className="titlebar-right">
          {isSettings ? (
            <button className="tb-btn" title="返回主界面" onClick={() => goToPage("main")}>
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M10 3L4 8l6 5" />
              </svg>
            </button>
          ) : (
            <button className="tb-btn" title="设置" onClick={() => goToPage("settings")}>
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="4.5" cy="4" r="1.3" />
                <line x1="7" y1="4" x2="14" y2="4" />
                <circle cx="4.5" cy="8" r="1.3" />
                <line x1="7" y1="8" x2="14" y2="8" />
                <circle cx="4.5" cy="12" r="1.3" />
                <line x1="7" y1="12" x2="14" y2="12" />
              </svg>
            </button>
          )}
          <button className="tb-btn tb-btn-close" title="关闭窗口" onClick={hideApp}>
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
              <path d="M1 1l8 8M9 1L1 9" />
            </svg>
          </button>
        </div>
      </div>
    );
  }

  // ── Main Page ────────────────────────────────────────────────────
  if (page === "main") {
    return (
      <main>
        <Titlebar />
        <div className="list-area">
          {/* Header: search + filters + clear */}
          <div className="list-area-header">
            <div className="search-wrap">
              <input
                type="text"
                className="search-input"
                placeholder="搜索剪贴板历史..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>

            <div className="filter-row">
              <div className="filter-tabs">
                <button
                  className={`filter-tab filter-tab-all${filter.kind === "all" ? " active" : ""}`}
                  onClick={() => { setFilter({ kind: "all" }); setOffset(0); setHasMore(true); }}
                >
                  全部
                </button>
                {TYPE_GROUPS.map((tg) => (
                  <button
                    key={tg.key}
                    className={`filter-tab filter-tab-${tg.key}${
                      filter.kind === "type" && filter.content_type === tg.key ? " active" : ""
                    }`}
                    onClick={() => { setFilter({ kind: "type", content_type: tg.key }); setOffset(0); setHasMore(true); }}
                  >
                    {tg.label}
                  </button>
                ))}
                {customGroups.map((cg) => {
                  const isActive = filter.kind === "group" && filter.group_id === cg.id;
                  return (
                    <button
                      key={cg.id}
                      className={`filter-tab filter-tab-custom${isActive ? " active" : ""}`}
                      style={isActive && cg.color ? { background: cg.color, borderColor: cg.color, color: isLightColor(cg.color) ? "#222" : "#fff" } : undefined}
                      onClick={() => { setFilter({ kind: "group", group_id: cg.id, group_name: cg.name }); setOffset(0); setHasMore(true); }}
                    >
                      {cg.name}
                    </button>
                  );
                })}
              </div>
              <button
                className="clear-all-btn"
                title="清空全部历史记录"
                onClick={() => setConfirmClear(true)}
              >
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M2 4h12" />
                  <path d="M5 4V3a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v1" />
                  <path d="M13 4v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4" />
                  <path d="M6 7v5" />
                  <path d="M10 7v5" />
                </svg>
                清空
              </button>
            </div>
          </div>

          {/* Toast inside list-area for relative positioning */}
          {popup && <div className={`popup-toast${popupError ? " error" : ""}`}>{popup}</div>}

          {/* Confirm dialog */}
          {confirmClear && (
            <div className="confirm-overlay" onClick={() => setConfirmClear(false)}>
              <div className="confirm-dialog" onClick={(e) => e.stopPropagation()}>
                <p className="confirm-msg">确认清空全部历史记录？</p>
                <p className="confirm-sub">此操作不可撤销</p>
                <div className="confirm-actions">
                  <button className="confirm-cancel" onClick={() => setConfirmClear(false)}>取消</button>
                  <button className="confirm-ok" onClick={() => { setConfirmClear(false); clearAll(); }}>确认清空</button>
                </div>
              </div>
            </div>
          )}

          {/* Item list */}
          {items.length === 0 ? (
            <div className="empty">{search ? "无匹配结果" : "暂无剪贴板历史"}</div>
          ) : (
            <ul className="item-list" ref={listScrollRef}>
              {items.map((item) => (
                <ListItem
                  key={item.id}
                  item={item}
                  imageCache={imageCache}
                  customGroups={customGroups}
                  showGroupTag={filter.kind !== "group"}
                  onCopyAndPaste={copyAndPaste}
                  onCopyAndPasteImage={copyAndPasteImage}
                  onDelete={deleteItem}
                  onMoveToGroup={moveToGroup}
                />
              ))}
              {!search && hasMore && (
                <li className="load-more-li">
                  <span className="load-more-hint">加载中...</span>
                </li>
              )}
            </ul>
          )}

          {/* Status bar */}
          {items.length > 0 && (
            <div className="status-bar">
              <span className="status-count">{items.length} 条记录</span>
            </div>
          )}
        </div>
      </main>
    );
  }

  // ── Settings Page ─────────────────────────────────────────────────
  return (
    <main>
      <Titlebar />
      <SettingsPage
        showPopup={showPopup}
        popup={popup}
        popupError={popupError}
        onSettingsChanged={() => setRefreshSig((s) => s + 1)}
      />
    </main>
  );
}

// ═══════════════════════════════════════════════════════════════════
// COLOR PICKER SWATCH
// ═══════════════════════════════════════════════════════════════════
const GROUP_PRESET_COLORS = [
  "#e74c3c", "#e67e22", "#f1c40f", "#2ecc71",
  "#1abc9c", "#3498db", "#9b59b6", "#e91e63",
  "#795548", "#607d8b",
];

function ColorPickerSwatch({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  const [showPicker, setShowPicker] = useState(false);
  const [hexInput, setHexInput] = useState(value || "");
  const [panelPos, setPanelPos] = useState<{ bottom: number; right: number }>({ bottom: 0, right: 0 });
  const pickerRef = useRef<HTMLDivElement>(null);
  const dotRef = useRef<HTMLButtonElement>(null);

  useEffect(() => { setHexInput(value || ""); }, [value]);

  useEffect(() => {
    if (!showPicker) return;
    function handleClick(e: MouseEvent) {
      if (pickerRef.current && !pickerRef.current.contains(e.target as Node)) setShowPicker(false);
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [showPicker]);

  function handlePreset(c: string) { setHexInput(c); onChange(c); setShowPicker(false); }

  function handleHexChange(raw: string) {
    const v = raw.trim();
    setHexInput(v);
    if (/^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(v)) onChange(v);
    else if (v === "") onChange("");
  }

  function handleNativeChange(e: React.ChangeEvent<HTMLInputElement>) {
    const c = e.target.value;
    setHexInput(c); onChange(c);
  }

  return (
    <div className="color-swatch-wrapper" ref={pickerRef}>
      <button
        ref={dotRef}
        className="color-swatch-dot"
        style={{ background: value || "#555" }}
        onClick={(e) => {
          e.stopPropagation();
          const nextShow = !showPicker;
          if (nextShow && dotRef.current) {
            const rect = dotRef.current.getBoundingClientRect();
            setPanelPos({
              bottom: window.innerHeight - rect.top + 12,
              right: window.innerWidth - rect.right - 20,
            });
          }
          setShowPicker(nextShow);
        }}
        title={value || "选择颜色"}
      />
      {showPicker && (
        <div className="color-picker-panel" style={{ position: "fixed", bottom: panelPos.bottom, right: panelPos.right }} onClick={(e) => e.stopPropagation()}>
          <div className="color-presets">
            {GROUP_PRESET_COLORS.map((c) => (
              <button
                key={c}
                className={`color-preset-swatch${hexInput.toLowerCase() === c ? " active" : ""}`}
                style={{ background: c }}
                onClick={() => handlePreset(c)}
                title={c}
              />
            ))}
          </div>
          <div className="color-hex-row">
            <input type="text" className="color-hex-input" value={hexInput} placeholder="#ff5722" maxLength={7} onChange={(e) => handleHexChange(e.target.value)} onClick={(e) => e.stopPropagation()} />
            <input type="color" className="color-native-picker" value={hexInput || "#555555"} onChange={handleNativeChange} onClick={(e) => e.stopPropagation()} />
            <button className="color-clear-btn" onClick={(e) => { e.stopPropagation(); setHexInput(""); onChange(""); setShowPicker(false); }}>无</button>
          </div>
        </div>
      )}
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════════
// SETTINGS PAGE
// ═══════════════════════════════════════════════════════════════════
function SettingsPage({
  showPopup,
  popup,
  popupError,
  onSettingsChanged,
}: {
  showPopup: (msg: string, isError?: boolean) => void;
  popup: string;
  popupError: boolean;
  onSettingsChanged: () => void;
}) {
  const [activeSection, setActiveSection] = useState<"general" | "groups">("general");

  // -- shortcut --
  const [shortcut, setShortcut] = useState(() => localStorage.getItem(SHORTCUT_STORAGE_KEY) || DEFAULT_SHORTCUT);
  const [recording, setRecording] = useState(false);

  // -- limits --
  const [maxItems, setMaxItems] = useState("");
  const [maxRetention, setMaxRetention] = useState("");

  // -- db path --
  const [dbPath, setDbPath] = useState("");

  // -- autostart --
  const [autoStart, setAutoStart] = useState(false);

  // -- groups --
  const [groups, setGroups] = useState<CustomGroup[]>([]);
  const [newGroupName, setNewGroupName] = useState("");
  const [newGroupColor, setNewGroupColor] = useState<string>("");
  const [editingGroupId, setEditingGroupId] = useState<number | null>(null);
  const [editGroupName, setEditGroupName] = useState("");
  const [editingGroupColor, setEditingGroupColor] = useState<string>("");

  // Load all config on mount
  useEffect(() => {
    invoke<string>("get_shortcut").then(setShortcut).catch(() => {});
    invoke<string>("get_db_path").then(setDbPath).catch(() => {});
    invoke<string | null>("get_config_value", { key: "max_items" }).then((v) => setMaxItems(v || "0")).catch(() => {});
    invoke<string | null>("get_config_value", { key: "max_retention_days" }).then((v) => setMaxRetention(v || "0")).catch(() => {});
    invoke<boolean>("get_autostart").then(setAutoStart).catch(() => {});
    invoke<CustomGroup[]>("list_groups").then(setGroups).catch(() => {});
  }, []);

  // ── keyboard recording ─────────────────────────────────────────
  useEffect(() => {
    if (!recording) return;
    function onKeyDown(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.altKey || e.metaKey || e.shiftKey)) return;
      const key = e.key;
      if (key === "Control" || key === "Shift" || key === "Alt" || key === "Meta") return;
      e.preventDefault(); e.stopPropagation(); e.stopImmediatePropagation();

      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Control");
      if (e.metaKey) parts.push("Super");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      parts.push(key.length === 1 ? key.toUpperCase() : key);

      const combo = parts.join("+");
      setShortcut(combo);
      showPopup(`已捕获: ${formatShortcut(combo)} — 点"应用"生效`);
      function onKeyUp(e2: KeyboardEvent) { e2.preventDefault(); e2.stopPropagation(); window.removeEventListener("keyup", onKeyUp, true); }
      window.addEventListener("keyup", onKeyUp, true);
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording]);

  function formatShortcut(s: string) {
    return s.replace(/CommandOrControl/g, "Ctrl").replace(/CmdOrCtrl/g, "Ctrl")
      .replace(/Control/g, "Ctrl").replace(/Command/g, "\u2318")
      .replace(/Shift/g, "\u21E7").replace(/Alt/g, "Alt")
      .replace(/Super/g, "\u229E").replace(/Meta/g, "\u229E");
  }

  async function commitShortcut(newShortcut: string) {
    if (!newShortcut || newShortcut.trim().length < 2) { showPopup("快捷键格式无效", true); return; }
    setRecording(false);
    try {
      await invoke("set_shortcut", { shortcut: newShortcut });
      localStorage.setItem(SHORTCUT_STORAGE_KEY, newShortcut);
      setShortcut(newShortcut);
      showPopup(`✓ 快捷键已设为 ${formatShortcut(newShortcut)}`);
    } catch (e: any) {
      const msg = String(e);
      if (msg.includes("已被其他程序占用")) showPopup("✗ 该快捷键已被其他程序占用，请尝试其他组合", true);
      else showPopup("✗ 注册失败，请尝试其他组合（可手动输入）", true);
    }
  }

  async function restoreDefault() { setShortcut(DEFAULT_SHORTCUT); await commitShortcut(DEFAULT_SHORTCUT); }

  async function toggleAutostart(val: boolean) {
    setAutoStart(val);
    try { await invoke("set_autostart", { enable: val }); showPopup(val ? "✓ 已开启开机自启动" : "✓ 已关闭开机自启动"); }
    catch (e) { showPopup(`✗ 操作失败: ${String(e)}`, true); setAutoStart(!val); }
  }

  async function saveLimits() {
    const m = parseInt(maxItems || "0", 10) || 0;
    const d = parseInt(maxRetention || "0", 10) || 0;
    try {
      await invoke("set_config_value", { key: "max_items", value: String(m) });
      await invoke("set_config_value", { key: "max_retention_days", value: String(d) });
      showPopup("✓ 限制配置已保存");
    } catch (e) { showPopup("✗ 保存失败", true); }
  }

  const dbFolder = dbPath ? dbPath.replace(/[\\/]clipforge\.db$/i, "").replace(/[\\/]$/, "") : "";
  const dbFolderShort = dbFolder ? (() => {
    const parts = dbFolder.split(/[\\/]/);
    return parts[parts.length - 1] || dbFolder;
  })() : "";

  async function pickFolder() {
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择数据库存储文件夹" });
      if (selected && typeof selected === "string") {
        const folder = (selected as string).replace(/[\\/]$/, "");
        const sep = folder.includes("/") ? "/" : "\\";
        setDbPath(`${folder}${sep}clipboard.db`);
      }
    } catch (e) { showPopup("选择文件夹失败", true); }
  }

  async function saveDbPath() {
    try { await invoke("set_db_path", { path: dbPath }); showPopup("✓ 数据库路径已保存，重启后生效"); }
    catch (e) { showPopup("✗ 保存失败", true); }
  }

  async function restoreDefaultDbPath() {
    setDbPath("");
    try { await invoke("set_db_path", { path: "" }); showPopup("✓ 已恢复默认路径（需重启）"); }
    catch (e) { showPopup("✗ 恢复失败", true); }
  }

  async function addGroup() {
    const name = newGroupName.trim();
    if (!name) return;
    try {
      const g = await invoke<CustomGroup>("create_group", { name, color: newGroupColor || null });
      setGroups((prev) => [...prev, g]);
      setNewGroupName(""); setNewGroupColor("");
      onSettingsChanged();
      showPopup(`✓ 已创建分组「${name}」`);
    } catch (e) { showPopup(`✗ 创建失败: ${String(e)}`, true); }
  }

  async function renameGroup(id: number) {
    const name = editGroupName.trim();
    if (!name) { setEditingGroupId(null); return; }
    try {
      await invoke("rename_group", { groupId: id, name });
      setGroups((prev) => prev.map((g) => (g.id === id ? { ...g, name } : g)));
      setEditingGroupId(null); onSettingsChanged();
      showPopup(`✓ 已重命名为「${name}」`);
    } catch (e) { showPopup(`✗ 重命名失败: ${String(e)}`, true); }
  }

  async function updateGroupColor(id: number, color: string) {
    try {
      await invoke("update_group_color", { groupId: id, color: color || null });
      setGroups((prev) => prev.map((g) => (g.id === id ? { ...g, color: color || undefined } : g)));
      onSettingsChanged();
    } catch (e) { showPopup(`✗ 颜色保存失败: ${String(e)}`, true); }
  }

  async function removeGroup(id: number) {
    try {
      await invoke("delete_group", { groupId: id });
      setGroups((prev) => prev.filter((g) => g.id !== id));
      onSettingsChanged();
      showPopup("✓ 已删除分组");
    } catch (e) { showPopup(`✗ 删除失败: ${String(e)}`, true); }
  }

  return (
    <div className="settings-layout">
      {/* Sidebar */}
      <div className="settings-sidebar">
        <button
          className={`settings-nav-item${activeSection === "general" ? " active" : ""}`}
          onClick={() => setActiveSection("general")}
        >
          <span className="settings-nav-dot" style={{ background: "var(--brand-500)" }} />
          常规设置
        </button>
        <button
          className={`settings-nav-item${activeSection === "groups" ? " active" : ""}`}
          onClick={() => setActiveSection("groups")}
        >
          <span className="settings-nav-dot" style={{ background: "var(--color-success)" }} />
          自定义分组
        </button>
      </div>

      {/* Panel */}
      <div className="settings-panel">
        {/* ── General Settings ── */}
        <div className={`settings-section${activeSection === "general" ? " active" : ""}`}>
          {/* Autostart */}
          <div className="settings-group">
            <div className="settings-group-header">
              <div className="settings-group-icon teal">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="8" cy="8" r="5.5" /><path d="M8 4v4l3 2" />
                </svg>
              </div>
              开机自启
            </div>
            <div className="settings-row">
              <div>
                <div className="settings-label-text">开机自动启动</div>
                <div className="settings-label-hint">系统启动时在后台运行 ClipForge</div>
              </div>
              <div className="toggle-wrap">
                <button
                  className={`toggle-track${autoStart ? " on" : ""}`}
                  onClick={() => toggleAutostart(!autoStart)}
                  role="switch"
                  aria-checked={autoStart}
                >
                  <span className="toggle-thumb" />
                </button>
              </div>
            </div>
          </div>

          {/* Shortcut */}
          <div className="settings-group">
            <div className="settings-group-header">
              <div className="settings-group-icon amber">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="1.5" y="3.5" width="13" height="9" rx="1.5" /><path d="M4.5 7.5h2M4.5 10h5" />
                </svg>
              </div>
              全局快捷键
            </div>
            {/* Row 1: label + shortcut badge */}
            <div className="settings-row">
              <div>
                <div className="settings-label-text">呼出快捷键</div>
              </div>
              <div
                className={`shortcut-badge${recording ? " recording" : ""}`}
                onClick={() => { setRecording(true); showPopup("按下组合键录制，或手动输入后点「应用」"); }}
                title="点击录制"
              >
                {shortcut}
              </div>
            </div>
            {/* Row 2: restore default (left) + apply (right) */}
            <div className="settings-row" style={{ borderBottom: "none" }}>
              <button className="btn-sm btn-sm-secondary" onClick={restoreDefault}>恢复默认</button>
              <button className="btn-sm btn-sm-primary" onClick={() => commitShortcut(shortcut)}>应用</button>
            </div>
          </div>

          {/* Storage */}
          <div className="settings-group">
            <div className="settings-group-header">
              <div className="settings-group-icon purple">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <ellipse cx="8" cy="5.5" rx="6.5" ry="3" /><path d="M1.5 5.5v5c0 1.66 2.91 3 6.5 3s6.5-1.34 6.5-3v-5" />
                </svg>
              </div>
              数据存储路径
            </div>
            <div className="settings-row" style={{ flexDirection: "column", alignItems: "stretch" }}>
              <div className="path-display" title={dbFolder ? `完整路径: ${dbFolder}` : "当前使用默认路径（AppData 目录）"}>{dbFolderShort || "（使用默认路径）"}</div>
            </div>
            <div className="settings-row" style={{ borderBottom: "none" }}>
              <div>
                <div className="settings-label-hint">修改后需重启应用生效</div>
              </div>
              <div style={{ display: "flex", gap: 6 }}>
                <button className="btn-sm btn-sm-secondary" onClick={restoreDefaultDbPath}>恢复默认</button>
                <button className="btn-sm btn-sm-secondary" onClick={pickFolder}>选择</button>
                <button className="btn-sm btn-sm-primary" onClick={saveDbPath}>保存</button>
              </div>
            </div>
          </div>

          {/* History Limits */}
          <div className="settings-group">
            <div className="settings-group-header">
              <div className="settings-group-icon blue">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="2" y="3" width="12" height="11" rx="1.5" /><path d="M5 7h6M5 10h4" />
                </svg>
              </div>
              历史记录限制
            </div>
            <div className="settings-row">
              <div>
                <div className="settings-label-text">最多保留条数</div>
                <div className="settings-label-hint">0 = 不限制（仅对未分组记录生效）</div>
              </div>
              <input className="mini-input" type="number" value={maxItems} min="0" onChange={(e) => setMaxItems(e.target.value)} />
            </div>
            <div className="settings-row">
              <div>
                <div className="settings-label-text">最长保存天数</div>
                <div className="settings-label-hint">0 = 永久保留</div>
              </div>
              <input className="mini-input" type="number" value={maxRetention} min="0" onChange={(e) => setMaxRetention(e.target.value)} />
            </div>
            <div className="settings-row" style={{ borderBottom: "none", justifyContent: "flex-end" }}>
              <button className="btn-sm btn-sm-primary" onClick={saveLimits}>应用</button>
            </div>
          </div>
        </div>

        {/* ── Custom Groups ── */}
        <div className={`settings-section${activeSection === "groups" ? " active" : ""}`}>
          <div className="settings-group">
            <div className="settings-group-header">
              <div className="settings-group-icon green">
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M2 4a1 1 0 0 1 1-1h3.2a1 1 0 0 1 .78.38L7.8 4.6a1 1 0 0 0 .78.4H13a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4Z" />
                </svg>
              </div>
              自定义分组
            </div>
            <div className="settings-label-hint" style={{ marginBottom: 8 }}>加入分组的记录不受自动清理限制，会永久保留</div>

            {/* Existing groups */}
            {groups.length > 0 && (
              <ul className="group-list">
                {groups.map((g) => (
                  <li
                    key={g.id}
                    className={`group-item${editingGroupId === g.id ? " editing" : ""}`}
                    onClick={() => {
                      if (editingGroupId !== g.id) { setEditingGroupId(g.id); setEditGroupName(g.name); setEditingGroupColor(g.color || ""); }
                    }}
                  >
                    {editingGroupId === g.id ? (
                      <>
                        <input
                          type="text" className="group-edit-input"
                          value={editGroupName} autoFocus
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => setEditGroupName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") renameGroup(g.id);
                            if (e.key === "Escape") { setEditingGroupId(null); setEditGroupName(""); setEditingGroupColor(""); }
                          }}
                          onBlur={() => renameGroup(g.id)}
                        />
                        <ColorPickerSwatch value={editingGroupColor} onChange={(c) => { setEditingGroupColor(c); updateGroupColor(g.id, c); }} />
                      </>
                    ) : (
                      <>
                        <span className="group-name" style={g.color ? { color: g.color, fontWeight: 600 } : undefined}>{g.name}</span>
                        <ColorPickerSwatch value={g.color || ""} onChange={(c) => updateGroupColor(g.id, c)} />
                      </>
                    )}
                    <div className="group-item-actions">
                      <button className="btn-sm btn-sm-secondary" style={{ color: "var(--color-danger)", borderColor: "oklch(55% 0.19 25 / 0.25)", padding: "3px 10px", fontSize: 11 }}
                        onClick={(e) => { e.stopPropagation(); removeGroup(g.id); }}>
                        删除
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}

            {/* Add group row */}
            <div className="group-add-row">
              <div style={{ width: 10, height: 10, borderRadius: "50%", background: newGroupColor || "var(--brand-400)", flexShrink: 0 }} />
              <input
                type="text" className="group-add-input"
                value={newGroupName} placeholder="输入分组名称..."
                onChange={(e) => setNewGroupName(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") addGroup(); }}
              />
              <ColorPickerSwatch value={newGroupColor} onChange={setNewGroupColor} />
              <button className="btn-sm btn-sm-primary" onClick={addGroup}>添加</button>
            </div>
          </div>
        </div>
      </div>

      {/* Popup */}
      {popup && <div className={`popup-toast${popupError ? " error" : ""}`}>{popup}</div>}
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════════
// LIST ITEM
// ═══════════════════════════════════════════════════════════════════
function ListItem({
  item,
  imageCache,
  customGroups,
  onCopyAndPaste,
  onCopyAndPasteImage,
  onDelete,
  onMoveToGroup,
  showGroupTag,
}: {
  item: ClipboardItem;
  imageCache: Record<string, string>;
  customGroups: CustomGroup[];
  onCopyAndPaste: (content: string) => void;
  onCopyAndPasteImage: (imagePath: string) => void;
  onDelete: (id: number) => void;
  onMoveToGroup: (itemId: number, groupId: number | null, groupName?: string | null) => void;
  showGroupTag: boolean;
}) {
  const ct = item.content_type;
  const typeLabel = TYPE_LABEL[ct] ?? "文本";
  const [groupMenu, setGroupMenu] = useState(false);
  const groupMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!groupMenu) return;
    function handleClick(e: MouseEvent) { if (groupMenuRef.current && !groupMenuRef.current.contains(e.target as Node)) setGroupMenu(false); }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [groupMenu]);

  useEffect(() => {
    if (!groupMenu) return;
    const unlisten = getCurrentWindow().listen("tauri://focus", () => setGroupMenu(false));
    return () => { unlisten.then((fn) => fn()); };
  }, [groupMenu]);

  const groupColor =
    item.group_id != null ? customGroups.find((g) => g.id === item.group_id)?.color ?? undefined : undefined;

  // Format time
  const timeText = item.created_at ? item.created_at.replace("T", " ").substring(0, 16) : "";

  return (
    <li
      className={`clip-item${ct === "image" ? " image-item" : ""}`}
      onClick={() => {
        if (ct === "image" && item.image_path) onCopyAndPasteImage(item.image_path);
        else if (ct !== "image") onCopyAndPaste(item.content);
      }}
    >
      {/* Tags row */}
      <div className="item-tags-row">
        <span className={`type-tag tag-${ct}`}>{typeLabel}</span>
        {showGroupTag && item.group_name && (
          <span className="type-tag tag-group" style={{ background: (groupColor || "#555") + "20", color: groupColor || "#555", borderColor: (groupColor || "#555") + "40" }}>
            <svg className="tag-icon" viewBox="0 0 14 14" fill="none" stroke={groupColor || "#555"} strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
              <path d="M2 3.5a1 1 0 0 1 1-1h2.5a1 1 0 0 1 .8.4l.5.6a1 1 0 0 0 .8.4H11a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3.5Z" />
            </svg>
            {item.group_name}
          </span>
        )}
      </div>

      {/* Content */}
      {ct === "image" ? (
        <div className="image-thumb">
          {imageCache[item.image_path] ? (
            <img src={imageCache[item.image_path]} alt="Clipboard" />
          ) : (
            <div className="image-loading">加载中...</div>
          )}
        </div>
      ) : (
        <div className="item-content">
          {ct === "color" ? (
            <span className="color-swatch">
              <span className="color-dot" style={{ background: item.content.trim() }} />
              {item.content.trim()}
            </span>
          ) : (item.content)}
        </div>
      )}

      {/* Meta */}
      <div className="item-meta">
        <span className="item-time">{timeText}</span>
        <div className="item-actions-right">
          {/* Group menu */}
          <div className="group-toggle">
            <button className="group-btn" title="分组" onClick={(e) => { e.stopPropagation(); setGroupMenu((v) => !v); }}>
              <svg viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" width="14" height="14">
                <path d="M2 3.5a1 1 0 0 1 1-1h2.5a1 1 0 0 1 .8.4l.5.6a1 1 0 0 0 .8.4H11a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3.5Z" />
              </svg>
            </button>
            {groupMenu && (
              <div className="group-dropdown" ref={groupMenuRef} onClick={(e) => e.stopPropagation()}>
                {item.group_id ? (
                  <button onClick={() => { onMoveToGroup(item.id, null, null); setGroupMenu(false); }}>
                    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" width="9" height="9"><path d="M1 11 11 1M1 1l10 10" /></svg>
                    {" "}移出分组
                  </button>
                ) : null}
                {customGroups.filter((g) => g.id !== item.group_id).map((g) => (
                  <button key={g.id} onClick={() => { onMoveToGroup(item.id, g.id, g.name); setGroupMenu(false); }}>{g.name}</button>
                ))}
                {customGroups.length === 0 && !item.group_id && (
                  <span className="group-empty-tip">暂无分组，请在设置中创建</span>
                )}
              </div>
            )}
          </div>
          <button className="delete-btn" onClick={(e) => { e.stopPropagation(); onDelete(item.id); }} title="删除此条">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M2 4h12M5 4V3a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v1M13 4v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4M6 7v5M10 7v5" />
            </svg>
          </button>
        </div>
      </div>
    </li>
  );
}

export default App;

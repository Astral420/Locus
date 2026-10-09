import { create } from "zustand";

export type ThemeMode = "system" | "light" | "dark";

export interface DownloadProgress {
  active: boolean;
  modelName: string;
  progress: number; // 0 - 100
  etaSeconds: number;
}

interface UiState {
  theme: ThemeMode;
  resolvedTheme: "light" | "dark";
  sidebarCollapsed: boolean;
  downloadStatus: DownloadProgress;
  activeRecoveryModal: boolean;
  
  // Actions
  setTheme: (theme: ThemeMode) => void;
  toggleSidebar: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  /** First-run "Offline-Ready Core Engine" notice: once closed it never returns. */
  coreEngineNoticeDismissed: boolean;
  dismissCoreEngineNotice: () => void;
  setDownloadStatus: (status: Partial<DownloadProgress>) => void;
  setActiveRecoveryModal: (open: boolean) => void;
}

const STORAGE_KEY_THEME = "locus_theme_preference";
const STORAGE_KEY_SIDEBAR = "locus_sidebar_collapsed";
const STORAGE_KEY_CORE_NOTICE = "locus_core_engine_notice_dismissed";

function resolveEffectiveTheme(theme: ThemeMode): "light" | "dark" {
  if (theme === "system") {
    if (typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches) {
      return "dark";
    }
    return "light";
  }
  return theme;
}

function applyThemeToDocument(theme: "light" | "dark") {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", theme);
  }
}

const initialTheme: ThemeMode = (typeof localStorage !== "undefined" &&
  (localStorage.getItem(STORAGE_KEY_THEME) as ThemeMode)) || "system";

const initialCoreNoticeDismissed: boolean =
  (typeof localStorage !== "undefined" && localStorage.getItem(STORAGE_KEY_CORE_NOTICE) === "true") || false;

const initialSidebar: boolean = (typeof localStorage !== "undefined" &&
  localStorage.getItem(STORAGE_KEY_SIDEBAR) === "true") || false;

const initialResolved = resolveEffectiveTheme(initialTheme);
applyThemeToDocument(initialResolved);

export const useUiStore = create<UiState>((set, get) => ({
  theme: initialTheme,
  resolvedTheme: initialResolved,
  sidebarCollapsed: initialSidebar,
  coreEngineNoticeDismissed: initialCoreNoticeDismissed,
  downloadStatus: {
    active: false,
    modelName: "",
    progress: 0,
    etaSeconds: 0,
  },
  activeRecoveryModal: false,

  setTheme: (newTheme) => {
    const resolved = resolveEffectiveTheme(newTheme);
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY_THEME, newTheme);
    }
    applyThemeToDocument(resolved);
    set({ theme: newTheme, resolvedTheme: resolved });
  },

  toggleSidebar: () => {
    const next = !get().sidebarCollapsed;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY_SIDEBAR, String(next));
    }
    set({ sidebarCollapsed: next });
  },

  dismissCoreEngineNotice: () => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY_CORE_NOTICE, "true");
    }
    set({ coreEngineNoticeDismissed: true });
  },

  setSidebarCollapsed: (collapsed) => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY_SIDEBAR, String(collapsed));
    }
    set({ sidebarCollapsed: collapsed });
  },

  setDownloadStatus: (status) =>
    set((state) => ({
      downloadStatus: { ...state.downloadStatus, ...status },
    })),

  setActiveRecoveryModal: (open) => set({ activeRecoveryModal: open }),
}));

// Listen for system color scheme changes when theme is set to 'system'
if (typeof window !== "undefined") {
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
    const { theme } = useUiStore.getState();
    if (theme === "system") {
      const resolved = e.matches ? "dark" : "light";
      applyThemeToDocument(resolved);
      useUiStore.setState({ resolvedTheme: resolved });
    }
  });
}

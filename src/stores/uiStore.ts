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
  setDownloadStatus: (status: Partial<DownloadProgress>) => void;
  setActiveRecoveryModal: (open: boolean) => void;
}

const STORAGE_KEY_THEME = "locus_theme_preference";
const STORAGE_KEY_SIDEBAR = "locus_sidebar_collapsed";

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

const initialSidebar: boolean = (typeof localStorage !== "undefined" &&
  localStorage.getItem(STORAGE_KEY_SIDEBAR) === "true") || false;

const initialResolved = resolveEffectiveTheme(initialTheme);
applyThemeToDocument(initialResolved);

export const useUiStore = create<UiState>((set, get) => ({
  theme: initialTheme,
  resolvedTheme: initialResolved,
  sidebarCollapsed: initialSidebar,
  downloadStatus: {
    active: true,
    modelName: "Llama 3.2 3B Instruct",
    progress: 45,
    etaSeconds: 85,
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

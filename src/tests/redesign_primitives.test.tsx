import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import React from "react";
import { Toggle } from "../components/ui/Toggle";
import { SelectPill } from "../components/ui/SelectPill";
import { SettingsCard, SettingsRow, SectionLabel } from "../components/ui/SettingsCard";
import { PathChip } from "../components/ui/PathChip";
import { Kbd, isMacPlatform } from "../components/ui/Kbd";
import { SegmentedControl } from "../components/ui/SegmentedControl";
import { ProgressBar } from "../components/ui/ProgressBar";
import { Popover } from "../components/ui/Popover";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("Redesign primitives: Toggle", () => {
  it("exposes role=switch with aria-checked and reports the next value", () => {
    const onChange = vi.fn();
    render(<Toggle label="Auto update" checked={false} onChange={onChange} />);
    const sw = screen.getByRole("switch", { name: "Auto update" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("reports false when switched off, and ignores clicks while disabled", () => {
    const onChange = vi.fn();
    const { rerender } = render(<Toggle label="x" checked onChange={onChange} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).toHaveBeenCalledWith(false);

    onChange.mockClear();
    rerender(<Toggle label="x" checked disabled onChange={onChange} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Redesign primitives: SelectPill", () => {
  it("is a labelled native select that reports the chosen value", () => {
    const onChange = vi.fn();
    render(
      <SelectPill
        aria-label="Theme"
        value="system"
        onChange={onChange}
        options={[
          { value: "system", label: "System" },
          { value: "dark", label: "Dark" },
        ]}
      />
    );
    const select = screen.getByLabelText("Theme") as HTMLSelectElement;
    expect(select.value).toBe("system");
    fireEvent.change(select, { target: { value: "dark" } });
    expect(onChange).toHaveBeenCalledWith("dark");
  });
});

describe("Redesign primitives: SettingsCard / SettingsRow", () => {
  it("renders title, label, muted description and a right-hand control", () => {
    render(
      <SettingsCard title="General">
        <SettingsRow label="App Version" description="Current build" control={<span>v0.1.0</span>} />
        <SettingsRow label="Env" control={<button>Edit</button>}>
          <input aria-label="inline child" />
        </SettingsRow>
      </SettingsCard>
    );
    expect(screen.getByRole("heading", { name: "General" })).toBeTruthy();
    expect(screen.getByText("App Version")).toBeTruthy();
    expect(screen.getByText("Current build")).toBeTruthy();
    expect(screen.getByText("v0.1.0")).toBeTruthy();
    expect(screen.getByLabelText("inline child")).toBeTruthy();
  });

  it("renders section labels as uppercase small text", () => {
    render(<SectionLabel>AI Providers</SectionLabel>);
    const el = screen.getByText("AI Providers");
    expect(el.className).toContain("uppercase");
    expect(el.className).toContain("text-[11px]");
  });
});

describe("Redesign primitives: PathChip", () => {
  it("copies the path to the clipboard and confirms", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<PathChip path="/Users/me/Locus/data" />);
    expect(screen.getByText("/Users/me/Locus/data")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
    expect(writeText).toHaveBeenCalledWith("/Users/me/Locus/data");
    await waitFor(() => expect(document.querySelector("svg.text-primary")).not.toBeNull());
  });
});

describe("Redesign primitives: Kbd", () => {
  it("shows the command symbol on macOS", () => {
    vi.spyOn(window.navigator, "platform", "get").mockReturnValue("MacIntel");
    expect(isMacPlatform()).toBe(true);
    render(<Kbd keys="N" />);
    expect(screen.getByText("⌘ N")).toBeTruthy();
  });

  it("shows Ctrl on other platforms and is hidden from assistive tech", () => {
    vi.spyOn(window.navigator, "platform", "get").mockReturnValue("Win32");
    vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue("Windows NT 10.0");
    expect(isMacPlatform()).toBe(false);
    render(<Kbd keys="K" />);
    const el = screen.getByText("Ctrl K");
    expect(el.getAttribute("aria-hidden")).toBe("true");
  });
});

describe("Redesign primitives: SegmentedControl", () => {
  it("marks the active option with aria-pressed and reports selection", () => {
    const onChange = vi.fn();
    render(
      <SegmentedControl
        aria-label="Kind"
        value="a"
        onChange={onChange}
        options={[
          { value: "a", label: "Alpha" },
          { value: "b", label: "Beta" },
        ]}
      />
    );
    expect(screen.getByRole("group", { name: "Kind" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Alpha" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Beta" }).getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Beta" }));
    expect(onChange).toHaveBeenCalledWith("b");
  });
});

describe("Redesign primitives: ProgressBar", () => {
  it("exposes progressbar semantics and clamps out-of-range and non-finite values", () => {
    const { rerender } = render(<ProgressBar value={42.4} label="dl" />);
    expect(screen.getByRole("progressbar", { name: "dl" }).getAttribute("aria-valuenow")).toBe("42");
    rerender(<ProgressBar value={250} label="dl" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
    rerender(<ProgressBar value={-5} label="dl" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("0");
    rerender(<ProgressBar value={Number.NaN} label="dl" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("0");
  });
});

describe("Redesign primitives: Popover", () => {
  it("renders nothing while closed", () => {
    render(
      <Popover open={false} onClose={() => undefined} aria-label="Panel">
        content
      </Popover>
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes on Escape and on an outside press, but not on an inside press", () => {
    const onClose = vi.fn();
    render(
      <div>
        <button>outside</button>
        <div className="relative">
          <Popover open onClose={onClose} aria-label="Panel">
            <span>inside</span>
          </Popover>
        </div>
      </div>
    );
    expect(screen.getByRole("dialog", { name: "Panel" })).toBeTruthy();

    fireEvent.mouseDown(screen.getByText("inside"));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByText("outside"));
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

describe("Redesign primitives: Button and Badge", () => {
  it("renders pill buttons with accessible variants", () => {
    const { rerender } = render(<Button>Go</Button>);
    const btn = screen.getByRole("button", { name: "Go" });
    expect(btn.className).toContain("rounded-full");
    expect(btn.className).toContain("text-on-primary");

    rerender(<Button variant="danger">Reset</Button>);
    expect(screen.getByRole("button", { name: "Reset" }).className).toContain("bg-status-error");

    rerender(<Button variant="secondary">Cancel</Button>);
    expect(screen.getByRole("button", { name: "Cancel" }).className).toContain("border-border");
  });

  it("disables the button and sets aria-busy while loading", () => {
    render(<Button isLoading>Save</Button>);
    const btn = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.getAttribute("aria-busy")).toBe("true");
  });

  it("renders tinted, borderless badges", () => {
    render(<Badge variant="green">Fits</Badge>);
    const badge = screen.getByText("Fits");
    expect(badge.className).toContain("bg-green-tint");
    expect(badge.className).not.toContain("border");
  });
});

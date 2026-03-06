"use client";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  FileText,
  Code2,
  FileCode,
  AlignLeft,
  Link2,
  Tags,
  Sparkles,
  type LucideIcon,
} from "lucide-react";

export interface ScrapeState {
  formats: string[];
  only_main_content: boolean;
  include_links: boolean;
  timeout_ms: string;
  ai_summary: boolean;
}

interface ScrapeOptionsProps {
  state: ScrapeState;
  onChange: (state: ScrapeState) => void;
}

const FORMAT_OPTIONS: {
  value: string;
  label: string;
  description: string;
  icon: LucideIcon;
}[] = [
  { value: "markdown", label: "Markdown", description: "Clean, readable text with formatting", icon: FileText },
  { value: "html", label: "HTML", description: "Cleaned HTML with main content", icon: Code2 },
  { value: "rawhtml", label: "Raw HTML", description: "Original unprocessed HTML source", icon: FileCode },
  { value: "content", label: "Content", description: "Plain text without any markup", icon: AlignLeft },
  { value: "links", label: "Links", description: "All hyperlinks found on the page", icon: Link2 },
  { value: "metadata", label: "Metadata", description: "Title, description, OG tags, etc.", icon: Tags },
];

export function ScrapeOptions({ state, onChange }: ScrapeOptionsProps) {
  const toggle = (value: string) => {
    const formats = state.formats.includes(value)
      ? state.formats.filter((f) => f !== value)
      : [...state.formats, value];
    onChange({ ...state, formats });
  };

  return (
    <div className="space-y-5">
      <div className="space-y-3">
        <Label className="text-xs text-muted-foreground uppercase tracking-wide">
          Output Formats
        </Label>
        <div className="space-y-1">
          {FORMAT_OPTIONS.map(({ value, label, description, icon: Icon }) => (
            <button
              key={value}
              type="button"
              onClick={() => toggle(value)}
              className="flex items-center gap-3 w-full rounded-md px-3 py-2.5 text-left transition-colors hover:bg-muted/50 cursor-pointer"
            >
              <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">{label}</p>
                <p className="text-xs text-muted-foreground">{description}</p>
              </div>
              <Switch
                checked={state.formats.includes(value)}
                onCheckedChange={() => toggle(value)}
                className="shrink-0"
              />
            </button>
          ))}
        </div>
      </div>

      <div className="space-y-3 border-t pt-4">
        <Label className="text-xs text-muted-foreground uppercase tracking-wide">
          AI
        </Label>

        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Sparkles className="h-4 w-4 text-muted-foreground" />
            <div>
              <Label htmlFor="ai-summary" className="text-sm font-medium">
                AI Summary
              </Label>
              <p className="text-xs text-muted-foreground">
                Generate a TL;DR using Claude Haiku
              </p>
            </div>
          </div>
          <Switch
            id="ai-summary"
            checked={state.ai_summary}
            onCheckedChange={(v) =>
              onChange({ ...state, ai_summary: v })
            }
          />
        </div>
      </div>

      <div className="space-y-3 border-t pt-4">
        <Label className="text-xs text-muted-foreground uppercase tracking-wide">
          Options
        </Label>

        <div className="flex items-center justify-between">
          <div>
            <Label htmlFor="main-content" className="text-sm font-medium">
              Main content only
            </Label>
            <p className="text-xs text-muted-foreground">
              Exclude navigation, footer, sidebar
            </p>
          </div>
          <Switch
            id="main-content"
            checked={state.only_main_content}
            onCheckedChange={(v) =>
              onChange({ ...state, only_main_content: v })
            }
          />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <Label htmlFor="include-links" className="text-sm font-medium">
              Include links
            </Label>
            <p className="text-xs text-muted-foreground">
              Extract all links found on the page
            </p>
          </div>
          <Switch
            id="include-links"
            checked={state.include_links}
            onCheckedChange={(v) =>
              onChange({ ...state, include_links: v })
            }
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="timeout" className="text-sm font-medium">
            Timeout (ms)
          </Label>
          <Input
            id="timeout"
            type="number"
            min="1000"
            max="120000"
            value={state.timeout_ms}
            onChange={(e) =>
              onChange({ ...state, timeout_ms: e.target.value })
            }
            className="w-full"
          />
        </div>
      </div>
    </div>
  );
}

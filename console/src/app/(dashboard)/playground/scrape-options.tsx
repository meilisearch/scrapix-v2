"use client";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  FileText,
  Code2,
  FileCode,
  AlignLeft,
  Link2,
  Tags,
} from "lucide-react";

export interface ScrapeState {
  formats: string[];
  only_main_content: boolean;
  include_links: boolean;
  timeout_ms: string;
}

interface ScrapeOptionsProps {
  state: ScrapeState;
  onChange: (state: ScrapeState) => void;
}

const FORMAT_OPTIONS = [
  { value: "markdown", label: "Markdown", icon: FileText },
  { value: "html", label: "HTML", icon: Code2 },
  { value: "rawhtml", label: "Raw HTML", icon: FileCode },
  { value: "content", label: "Content", icon: AlignLeft },
  { value: "links", label: "Links", icon: Link2 },
  { value: "metadata", label: "Metadata", icon: Tags },
] as const;

export function ScrapeOptions({ state, onChange }: ScrapeOptionsProps) {
  return (
    <Tabs defaultValue="formats" className="h-full flex flex-col">
      <TabsList className="grid w-full grid-cols-2">
        <TabsTrigger value="formats">Formats</TabsTrigger>
        <TabsTrigger value="options">Options</TabsTrigger>
      </TabsList>

      <TabsContent value="formats" className="flex-1 space-y-4 pt-2">
        <div className="space-y-2">
          <Label className="text-xs text-muted-foreground uppercase tracking-wide">
            Output Formats
          </Label>
          <ToggleGroup
            type="multiple"
            variant="outline"
            value={state.formats}
            onValueChange={(formats) => onChange({ ...state, formats })}
            className="flex flex-wrap gap-2"
          >
            {FORMAT_OPTIONS.map(({ value, label, icon: Icon }) => (
              <ToggleGroupItem
                key={value}
                value={value}
                className="gap-1.5 px-3 py-1.5 data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
              >
                <Icon className="h-3.5 w-3.5" />
                {label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
      </TabsContent>

      <TabsContent value="options" className="flex-1 space-y-4 pt-2">
        <div className="space-y-4">
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
      </TabsContent>
    </Tabs>
  );
}

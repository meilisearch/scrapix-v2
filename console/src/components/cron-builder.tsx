"use client";

import { useState, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Badge } from "@/components/ui/badge";
import { Clock, Pencil } from "lucide-react";
import { cn } from "@/lib/utils";

interface CronBuilderProps {
  value: string;
  onChange: (value: string) => void;
}

interface CronPreset {
  label: string;
  value: string;
  description: string;
}

const PRESETS: CronPreset[] = [
  { label: "Every hour", value: "0 * * * *", description: "At minute 0 of every hour" },
  { label: "Every 6 hours", value: "0 */6 * * *", description: "At minute 0 every 6 hours" },
  { label: "Every 12 hours", value: "0 */12 * * *", description: "At minute 0 every 12 hours" },
  { label: "Daily at midnight", value: "0 0 * * *", description: "Every day at 00:00 UTC" },
  { label: "Daily at 2 AM", value: "0 2 * * *", description: "Every day at 02:00 UTC" },
  { label: "Daily at 6 AM", value: "0 6 * * *", description: "Every day at 06:00 UTC" },
  { label: "Twice daily", value: "0 0,12 * * *", description: "Every day at 00:00 and 12:00 UTC" },
  { label: "Weekly (Monday)", value: "0 0 * * 1", description: "Every Monday at 00:00 UTC" },
  { label: "Weekly (Sunday)", value: "0 0 * * 0", description: "Every Sunday at 00:00 UTC" },
  { label: "Monthly (1st)", value: "0 0 1 * *", description: "First of every month at 00:00 UTC" },
];

const WEEKDAYS = [
  { value: "*", label: "Every day" },
  { value: "1-5", label: "Weekdays" },
  { value: "0,6", label: "Weekends" },
  { value: "0", label: "Sunday" },
  { value: "1", label: "Monday" },
  { value: "2", label: "Tuesday" },
  { value: "3", label: "Wednesday" },
  { value: "4", label: "Thursday" },
  { value: "5", label: "Friday" },
  { value: "6", label: "Saturday" },
];

const MONTHS = [
  { value: "*", label: "Every month" },
  { value: "1", label: "January" },
  { value: "2", label: "February" },
  { value: "3", label: "March" },
  { value: "4", label: "April" },
  { value: "5", label: "May" },
  { value: "6", label: "June" },
  { value: "7", label: "July" },
  { value: "8", label: "August" },
  { value: "9", label: "September" },
  { value: "10", label: "October" },
  { value: "11", label: "November" },
  { value: "12", label: "December" },
];

function parseCron(expression: string): {
  minute: string;
  hour: string;
  day: string;
  month: string;
  weekday: string;
} {
  const parts = expression.trim().split(/\s+/);
  return {
    minute: parts[0] ?? "*",
    hour: parts[1] ?? "*",
    day: parts[2] ?? "*",
    month: parts[3] ?? "*",
    weekday: parts[4] ?? "*",
  };
}

function buildCron(parts: {
  minute: string;
  hour: string;
  day: string;
  month: string;
  weekday: string;
}): string {
  return `${parts.minute} ${parts.hour} ${parts.day} ${parts.month} ${parts.weekday}`;
}

function describeCron(expression: string): string {
  if (!expression.trim()) return "";

  const parts = expression.trim().split(/\s+/);
  if (parts.length !== 5) return "Invalid cron expression";

  // Check presets first
  const preset = PRESETS.find((p) => p.value === expression.trim());
  if (preset) return preset.description;

  const [minute, hour, day, month, weekday] = parts;

  const segments: string[] = [];

  // Time
  if (minute === "*" && hour === "*") {
    segments.push("Every minute");
  } else if (minute === "0" && hour === "*") {
    segments.push("Every hour");
  } else if (minute?.startsWith("*/")) {
    segments.push(`Every ${minute.slice(2)} minutes`);
  } else if (hour?.startsWith("*/")) {
    segments.push(`At minute ${minute} every ${hour.slice(2)} hours`);
  } else if (hour?.includes(",")) {
    const hours = hour.split(",").map((h) => `${h.padStart(2, "0")}:${(minute ?? "0").padStart(2, "0")}`);
    segments.push(`At ${hours.join(" and ")}`);
  } else if (hour !== "*") {
    segments.push(
      `At ${(hour ?? "0").padStart(2, "0")}:${(minute ?? "0").padStart(2, "0")} UTC`
    );
  } else {
    segments.push(`At minute ${minute}`);
  }

  // Day of month
  if (day !== "*") {
    if (day === "1") segments.push("on the 1st");
    else if (day === "15") segments.push("on the 15th");
    else segments.push(`on day ${day}`);
  }

  // Month
  if (month !== "*") {
    const monthObj = MONTHS.find((m) => m.value === month);
    if (monthObj) segments.push(`in ${monthObj.label}`);
    else segments.push(`in month ${month}`);
  }

  // Weekday
  if (weekday !== "*") {
    const dayObj = WEEKDAYS.find((d) => d.value === weekday);
    if (dayObj && dayObj.value !== "*") segments.push(`on ${dayObj.label}`);
    else segments.push(`on weekday ${weekday}`);
  }

  return segments.join(" ");
}

type Mode = "presets" | "custom" | "raw";

export function CronBuilder({ value, onChange }: CronBuilderProps) {
  const [mode, setMode] = useState<Mode>(() => {
    if (!value.trim()) return "presets";
    if (PRESETS.some((p) => p.value === value.trim())) return "presets";
    return "custom";
  });

  const parts = useMemo(() => parseCron(value || "0 0 * * *"), [value]);
  const description = useMemo(() => describeCron(value), [value]);

  const updatePart = (
    key: "minute" | "hour" | "day" | "month" | "weekday",
    val: string
  ) => {
    onChange(buildCron({ ...parts, [key]: val }));
  };

  const activePreset = PRESETS.find((p) => p.value === value.trim());

  return (
    <div className="space-y-3">
      {/* Mode selector */}
      <ToggleGroup
        type="single"
        variant="outline"
        value={mode}
        onValueChange={(v) => {
          if (v) setMode(v as Mode);
        }}
        className="w-full"
      >
        <ToggleGroupItem
          value="presets"
          className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
        >
          <Clock className="h-3 w-3 mr-1" />
          Presets
        </ToggleGroupItem>
        <ToggleGroupItem
          value="custom"
          className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
        >
          Custom
        </ToggleGroupItem>
        <ToggleGroupItem
          value="raw"
          className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
        >
          <Pencil className="h-3 w-3 mr-1" />
          Raw
        </ToggleGroupItem>
      </ToggleGroup>

      {/* Presets mode */}
      {mode === "presets" && (
        <div className="grid grid-cols-2 gap-1.5">
          {PRESETS.map((preset) => (
            <Button
              key={preset.value}
              type="button"
              variant="outline"
              size="sm"
              className={cn(
                "h-auto py-2 px-3 justify-start text-left",
                activePreset?.value === preset.value &&
                  "border-primary bg-primary/5 text-primary"
              )}
              onClick={() => onChange(preset.value)}
            >
              <div>
                <div className="text-xs font-medium">{preset.label}</div>
                <div className="text-[10px] text-muted-foreground font-normal">
                  {preset.description}
                </div>
              </div>
            </Button>
          ))}
        </div>
      )}

      {/* Custom mode */}
      {mode === "custom" && (
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            {/* Minute */}
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Minute</Label>
              <Select value={parts.minute} onValueChange={(v) => updatePart("minute", v)}>
                <SelectTrigger className="text-xs font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">0 (top of hour)</SelectItem>
                  <SelectItem value="15">15 (quarter past)</SelectItem>
                  <SelectItem value="30">30 (half past)</SelectItem>
                  <SelectItem value="45">45 (quarter to)</SelectItem>
                  <SelectItem value="*/5">*/5 (every 5 min)</SelectItem>
                  <SelectItem value="*/10">*/10 (every 10 min)</SelectItem>
                  <SelectItem value="*/15">*/15 (every 15 min)</SelectItem>
                  <SelectItem value="*/30">*/30 (every 30 min)</SelectItem>
                  <SelectItem value="*">* (every minute)</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {/* Hour */}
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Hour</Label>
              <Select value={parts.hour} onValueChange={(v) => updatePart("hour", v)}>
                <SelectTrigger className="text-xs font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="*">* (every hour)</SelectItem>
                  <SelectItem value="*/2">*/2 (every 2 hours)</SelectItem>
                  <SelectItem value="*/4">*/4 (every 4 hours)</SelectItem>
                  <SelectItem value="*/6">*/6 (every 6 hours)</SelectItem>
                  <SelectItem value="*/12">*/12 (every 12 hours)</SelectItem>
                  {Array.from({ length: 24 }, (_, i) => (
                    <SelectItem key={i} value={String(i)}>
                      {String(i).padStart(2, "0")}:00
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            {/* Day of month */}
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Day</Label>
              <Select value={parts.day} onValueChange={(v) => updatePart("day", v)}>
                <SelectTrigger className="text-xs font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="*">Every day</SelectItem>
                  <SelectItem value="1">1st</SelectItem>
                  <SelectItem value="15">15th</SelectItem>
                  <SelectItem value="1,15">1st & 15th</SelectItem>
                  {Array.from({ length: 31 }, (_, i) => (
                    <SelectItem key={i + 1} value={String(i + 1)}>
                      {i + 1}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Month */}
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Month</Label>
              <Select value={parts.month} onValueChange={(v) => updatePart("month", v)}>
                <SelectTrigger className="text-xs font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MONTHS.map((m) => (
                    <SelectItem key={m.value} value={m.value}>
                      {m.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Weekday */}
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">Weekday</Label>
              <Select value={parts.weekday} onValueChange={(v) => updatePart("weekday", v)}>
                <SelectTrigger className="text-xs font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {WEEKDAYS.map((d) => (
                    <SelectItem key={d.value} value={d.value}>
                      {d.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
      )}

      {/* Raw mode */}
      {mode === "raw" && (
        <div className="space-y-1.5">
          <Input
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder="0 2 * * *"
            className="font-mono text-sm"
          />
          <p className="text-[10px] text-muted-foreground">
            Format: minute hour day month weekday
          </p>
        </div>
      )}

      {/* Live preview */}
      {value.trim() && (
        <div className="flex items-center gap-2 rounded-md bg-muted/50 px-3 py-2">
          <Badge variant="outline" className="font-mono text-[10px] shrink-0">
            {value.trim()}
          </Badge>
          <span className="text-xs text-muted-foreground">{description}</span>
        </div>
      )}
    </div>
  );
}

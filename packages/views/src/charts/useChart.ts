// SPDX-License-Identifier: AGPL-3.0-or-later
/** What a chart reads (M8, ADR-0032): its rows from the store, its colours from the theme. */
import { useAggregate, type AggregateResult, type FieldRow } from "@databaseme/client";
import { useMemo } from "react";
import type { Listing } from "../framework/listing";
import { useThemeKey } from "../framework/useTheme";
import {
  chartAggregate,
  chartData,
  chartSpec,
  resolveChart,
  type ChartData,
  type ChartLook,
  type ChartSettings,
  type ChartSpec,
  type ResolvedChart,
} from "./chart";

export function useChartLook(): ChartLook {
  const theme = useThemeKey();
  return useMemo(() => {
    const style = getComputedStyle(document.documentElement);
    const token = (name: string) => style.getPropertyValue(`--dbme-${name}`).trim();
    return {
      series: Array.from({ length: 8 }, (_, slot) => token(`chart-${String(slot + 1)}`)),
      neutral: token("color-fg-muted"),
      surface: theme.trim(),
      text: token("color-fg"),
      muted: token("color-fg-muted"),
      grid: token("color-border"),
      font: getComputedStyle(document.body).fontFamily,
    };
  }, [theme]);
}

export interface ChartRead {
  chart: ResolvedChart;
  data: ChartData;
  ready: boolean;
  failed: boolean;
}

/** Reads what a chart's settings ask for. */
export function useChartRead(
  listing: Listing,
  settings: ChartSettings,
  fields: readonly FieldRow[],
): ChartRead {
  const chart = useMemo(() => resolveChart(settings, fields), [settings, fields]);
  const read = useAggregate(chartAggregate(chart, listing));
  const result: AggregateResult | undefined = read.data;
  const data = useMemo(() => chartData(chart, result), [chart, result]);
  return { chart, data, ready: read.isSuccess, failed: read.isError };
}

/** The spec a saved chart keeps: what it draws, without the vault's numbers or the screen's size. */
export function useStoredSpec(chart: ResolvedChart, data: ChartData): ChartSpec {
  const look = useChartLook();
  return useMemo(
    () => chartSpec(chart, data, look, { width: 640, height: 320 }),
    [chart, data, look],
  );
}

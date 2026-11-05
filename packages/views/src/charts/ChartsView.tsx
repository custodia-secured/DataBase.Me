// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Charts view (M8, Task 7, ADR-0032): a chart of what a source holds, with its settings beside it.
 * "Show as list" is the table of the rows the picture is drawn from.
 */
import { useFields, type FieldRow, type MeasureOp, type TimeBin } from "@databaseme/client";
import { useMediaQuery } from "@databaseme/ui";
import { useId, useMemo, type ReactNode } from "react";
import type { Listing } from "../framework/listing";
import { useViewState, type ViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import {
  BINS,
  chartTitle,
  DEFAULT_CHART,
  groupChoices,
  MARKS,
  marksFor,
  MEASURES,
  numberChoices,
  resolveChart,
  seriesChoices,
  type ChartMark,
  type ChartSettings,
  type ResolvedChart,
} from "./chart";
import { ChartBody } from "./ChartPanel";
import { useChartRead, useStoredSpec, type ChartRead } from "./useChart";

export function ChartsView({ objectId }: ViewProps) {
  const state = useViewState<ChartSettings>(objectId, DEFAULT_CHART);
  const fields = useFields(state.listing.source);
  return (
    <section className="dbme-charts" aria-label="Charts">
      {fields.isSuccess ? (
        <LoadedCharts
          state={state}
          objectId={objectId}
          listing={state.listing}
          fields={fields.data}
        />
      ) : null}
    </section>
  );
}

function LoadedCharts({
  state,
  objectId,
  listing,
  fields,
}: {
  state: ViewState<ChartSettings>;
  objectId: string;
  listing: Listing;
  fields: readonly FieldRow[];
}) {
  const read = useChartRead(listing, state.options, fields);
  const spec = useStoredSpec(read.chart, read.data);
  return (
    <>
      <ViewControls
        state={state}
        objectId={objectId}
        renderer="charts"
        alternative
        saveWith={() => ({ spec })}
      />
      <Settings state={state} fields={fields} chart={read.chart} />
      <Heading read={read} />
      <ChartBody
        read={read}
        asList={state.asList}
        empty="There is nothing to chart here yet. Objects with a value in this field will appear here."
      />
    </>
  );
}

function Heading({ read }: { read: ChartRead }) {
  return <h3 className="dbme-charts__title">{chartTitle(read.chart)}</h3>;
}

function Choice({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <span className="dbme-source">
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        className="dbme-input"
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      >
        {children}
      </select>
    </span>
  );
}

/** Six choices do not fit above a chart on a phone, so there they start folded away. */
function Settings({
  state,
  fields,
  chart,
}: {
  state: ViewState<ChartSettings>;
  fields: readonly FieldRow[];
  chart: ResolvedChart;
}) {
  const phone = useMediaQuery("(max-width: 767px)");
  const groups = useMemo(() => groupChoices(fields), [fields]);
  const numbers = useMemo(() => numberChoices(fields), [fields]);
  const splits = useMemo(
    () => seriesChoices(fields).filter((field) => field.key !== chart.x.key),
    [fields, chart.x.key],
  );
  const set = (change: Partial<ChartSettings>) => {
    // What is showing becomes what is chosen, so changing one thing leaves the others as they look.
    state.setOptions({
      mark: chart.mark,
      x: chart.x.key,
      bin: chart.bin,
      measure: chart.measure,
      of: chart.of?.key ?? null,
      series: chart.series?.key ?? null,
      ...change,
    });
  };
  const marks = marksFor(chart.x);
  const isTime = resolveChart({ ...DEFAULT_CHART, x: chart.x.key }, fields).x.kind;
  const byDate = isTime === "date" || isTime === "datetime" || isTime === "event";
  return (
    <details className="dbme-charts__settings" open={!phone}>
      <summary>Chart settings</summary>
      <div className="dbme-view-bar">
        <Choice
          label="Chart"
          value={chart.mark}
          onChange={(value) => {
            set({ mark: value as ChartMark });
          }}
        >
          {MARKS.filter((mark) => marks.includes(mark.id)).map((mark) => (
            <option key={mark.id} value={mark.id}>
              {mark.label}
            </option>
          ))}
        </Choice>
        <Choice
          label="Group by"
          value={chart.x.key}
          onChange={(value) => {
            set({
              x: value,
              series: chart.series?.key === value ? null : (chart.series?.key ?? null),
            });
          }}
        >
          {groups.map((field) => (
            <option key={field.key} value={field.key}>
              {field.label}
            </option>
          ))}
        </Choice>
        {byDate ? (
          <Choice
            label="Each"
            value={chart.bin}
            onChange={(value) => {
              set({ bin: value as TimeBin });
            }}
          >
            {BINS.map((bin) => (
              <option key={bin.id} value={bin.id}>
                {bin.label}
              </option>
            ))}
          </Choice>
        ) : null}
        <Choice
          label="Measure"
          value={chart.measure}
          onChange={(value) => {
            set({ measure: value as MeasureOp, of: numbers[0]?.key ?? null });
          }}
        >
          {MEASURES.filter((measure) => measure.id === "count" || numbers.length > 0).map(
            (measure) => (
              <option key={measure.id} value={measure.id}>
                {measure.label}
              </option>
            ),
          )}
        </Choice>
        {chart.of ? (
          <Choice
            label="Of"
            value={chart.of.key}
            onChange={(value) => {
              set({ of: value });
            }}
          >
            {numbers.map((field) => (
              <option key={field.key} value={field.key}>
                {field.label}
              </option>
            ))}
          </Choice>
        ) : null}
        {splits.length > 0 ? (
          <Choice
            label="Split by"
            value={chart.series?.key ?? ""}
            onChange={(value) => {
              set({ series: value === "" ? null : value });
            }}
          >
            <option value="">Nothing</option>
            {splits.map((field) => (
              <option key={field.key} value={field.key}>
                {field.label}
              </option>
            ))}
          </Choice>
        ) : null}
      </div>
    </details>
  );
}

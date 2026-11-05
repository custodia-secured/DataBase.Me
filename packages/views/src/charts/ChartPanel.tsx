// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A chart on screen (M8, ADR-0032): the read, the figure or its table, and what it has to say about
 * either. The Charts view is one of these with its settings beside it; a dashboard tile is another.
 *
 * It draws from settings, through `chart.ts`. Only rows counted in the store reach the picture, and a
 * stored spec is never read back, so a chart in a shared vault cannot make a request or run anything.
 */
import { useFields, type FieldRow } from "@databaseme/client";
import { StatusMessage } from "@databaseme/ui";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { Listing } from "../framework/listing";
import {
  chartSpec,
  chartSummary,
  chartTitle,
  measureTitle,
  type ChartData,
  type ChartSettings,
  type ChartSize,
  type ResolvedChart,
} from "./chart";
import { useChartLook, useChartRead, type ChartRead } from "./useChart";

interface Tip {
  x: number;
  y: number;
  lines: [string, string][];
}

/** The picture. Vega comes in with this view's chunk; nothing else in the app pays for it. */
function Figure({ chart, data, label }: { chart: ResolvedChart; data: ChartData; label: string }) {
  const look = useChartLook();
  const frame = useRef<HTMLDivElement | null>(null);
  const host = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState<ChartSize | null>(null);
  const [failed, setFailed] = useState(false);
  const [tip, setTip] = useState<Tip | null>(null);

  useLayoutEffect(() => {
    const element = frame.current;
    if (!element) return;
    const measure = () => {
      const { clientWidth: width, clientHeight: height } = element;
      setSize((before) =>
        before?.width === width && before.height === height ? before : { width, height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    const element = host.current;
    if (!element || !size || size.width === 0 || size.height === 0) return;
    let finalize: (() => void) | null = null;
    let stale = false;
    const draw = async () => {
      const [{ compile }, { parse, View }, { expressionInterpreter }] = await Promise.all([
        import("vega-lite"),
        import("vega"),
        import("vega-interpreter"),
      ]);
      if (stale) return;
      const spec = {
        ...chartSpec(chart, data, look, size),
        data: { values: data.rows },
        width: size.width,
        height: size.height,
        autosize: { type: "fit", contains: "padding" },
      };
      const compiled = compile(spec as unknown as Parameters<typeof compile>[0]).spec;
      const view = new View(parse(compiled, undefined, { ast: true }), {
        expr: expressionInterpreter,
        renderer: "svg",
        container: element,
        hover: false,
      });
      finalize = () => {
        view.finalize();
      };
      view.tooltip((_handler, event, _item, value) => {
        if (value === null || value === undefined || typeof value !== "object") {
          setTip(null);
          return;
        }
        setTip({
          x: event.clientX,
          y: event.clientY,
          lines: Object.entries(value as Record<string, unknown>).map(([key, shown]) => [
            key,
            String(shown),
          ]),
        });
      });
      await view.runAsync();
    };
    setFailed(false);
    draw().catch((error: unknown) => {
      if (!stale) {
        console.error(error);
        setFailed(true);
      }
    });
    return () => {
      stale = true;
      finalize?.();
      element.replaceChildren();
      setTip(null);
    };
  }, [chart, data, look, size]);

  return (
    <>
      <div
        ref={frame}
        className="dbme-charts__figure"
        role="img"
        aria-label={label}
        onPointerLeave={() => {
          setTip(null);
        }}
      >
        {/* Vega writes its own role and name on what it is given, so it gets this and not the figure. */}
        <div ref={host} className="dbme-charts__canvas" aria-hidden="true" />
      </div>
      {failed ? <StatusMessage tone="error">This chart could not be drawn.</StatusMessage> : null}
      {tip ? (
        <div
          className="dbme-charts__tip"
          style={{ left: `${String(tip.x + 12)}px`, top: `${String(tip.y + 12)}px` }}
        >
          {tip.lines.map(([key, value]) => (
            <div key={key}>
              <span className="dbme-muted">{key}</span> {value}
            </div>
          ))}
        </div>
      ) : null}
    </>
  );
}

/** The chart's rows, written out: every value the picture encodes, and what it folded away. */
function Table({ chart, data }: { chart: ResolvedChart; data: ChartData }) {
  return (
    <div
      className="dbme-charts__table"
      role="region"
      aria-label={`Data for ${chartTitle(chart)}`}
      tabIndex={0}
    >
      <table>
        <thead>
          <tr>
            <th scope="col">{chart.x.label}</th>
            {chart.series ? <th scope="col">{chart.series.label}</th> : null}
            <th scope="col" className="dbme-charts__number">
              {measureTitle(chart)}
            </th>
          </tr>
        </thead>
        <tbody>
          {data.table.map((row) => (
            <tr key={`${row.x}\u0000${row.series}`}>
              <td>{row.x}</td>
              {chart.series ? <td>{row.series}</td> : null}
              <td className="dbme-charts__number">
                {row.value === null ? "None" : row.value.toLocaleString()}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function ChartBody({
  read,
  asList,
  empty,
}: {
  read: ChartRead;
  asList: boolean;
  /** What to say when there is nothing to chart. */
  empty?: ReactNode;
}) {
  const { chart, data, ready, failed } = read;
  if (failed) return <StatusMessage tone="error">This chart could not be read.</StatusMessage>;
  if (!ready) return null;
  if (data.table.length === 0) {
    return <StatusMessage>{empty ?? "There is nothing to chart here yet."}</StatusMessage>;
  }
  return (
    <>
      {data.truncated ? (
        <StatusMessage>
          There are more groups than a chart can show, so this shows the first of them. A filter or
          a longer period brings the rest in.
        </StatusMessage>
      ) : null}
      {data.leftOut > 0 ? (
        <StatusMessage>
          {data.leftOut.toLocaleString()} more {data.leftOut === 1 ? "series is" : "series are"} in
          the list, as they cannot be told apart here.
        </StatusMessage>
      ) : null}
      {asList ? (
        <Table chart={chart} data={data} />
      ) : (
        <Figure chart={chart} data={data} label={chartSummary(chart, data)} />
      )}
    </>
  );
}

/** A chart that reads its own fields, for a tile with nothing else to do with them. */
export function ChartPanel({
  listing,
  settings,
  asList = false,
}: {
  listing: Listing;
  settings: ChartSettings;
  asList?: boolean;
}) {
  const fields = useFields(listing.source);
  if (!fields.isSuccess) return null;
  return <LoadedChart listing={listing} settings={settings} asList={asList} fields={fields.data} />;
}

function LoadedChart({
  listing,
  settings,
  asList,
  fields,
}: {
  listing: Listing;
  settings: ChartSettings;
  asList: boolean;
  fields: readonly FieldRow[];
}) {
  const read = useChartRead(listing, settings, fields);
  return <ChartBody read={read} asList={asList} />;
}

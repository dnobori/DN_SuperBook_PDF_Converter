#pragma warning disable CA2235 // Mark all non-serializable fields

using System;
using System.Buffers;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Diagnostics.CodeAnalysis;
using System.Runtime.Serialization;

using IPA.Cores.Basic;
using IPA.Cores.Helper.Basic;
using static IPA.Cores.Globals.Basic;

using IPA.Cores.Codes;
using IPA.Cores.Helper.Codes;
using static IPA.Cores.Globals.Codes;

using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.Formats.Jpeg;
using SixLabors.ImageSharp.Processing.Processors.Transforms;
using SixLabors.ImageSharp.Drawing;              // IPathBuilder, RectangleF など
using SixLabors.ImageSharp.Drawing.Processing;   // ctx.Draw(…) 拡張メソッド


using UglyToad.PdfPig;
using UglyToad.PdfPig.Content;
using UglyToad.PdfPig.Writer;
using UglyToad.PdfPig.DocumentLayoutAnalysis.TextExtractor;
using UglyToad.PdfPig.Tokens;


using OpenCvSharp;            // OpenCV 本体
using OpenCvSharp.Extensions;
using PointCv = OpenCvSharp.Point; // 混同防止のため、OpenCV の Point を別名に
using ImagePoint = SixLabors.ImageSharp.Point; // ImageSharp の Point
using Tesseract;

using SuperBookTools;
using SuperBookTools.App;
using SixLabors.ImageSharp.Formats;
using SixLabors.ImageSharp.ColorSpaces.Conversion;
using SixLabors.ImageSharp.ColorSpaces;
using Newtonsoft.Json;
using System.Security.Cryptography;
using Newtonsoft.Json.Linq;

namespace SuperBookTools;


[Flags]
public enum PdfYomitokuFormats
{
    Pdf = 0,
    MD,
    Html,
    JSON,
}

public class PdfYomitokuOptions
{
    public PdfYomitokuFormats Format;
    public int Dpi = 300;
    public bool LiteMode = false;
    public bool IgnoreLineBreak = true;
    public bool Combine = true;
    public bool IgnoreHeaderFooter = true;
    public string Device = "cuda";
    public bool OutputFigures = true;
    public bool OutputFigureLetters = true;
    public string Encoding = "utf-8-sig";
    public int TimeoutSecs = 5 * 3600;
}

public class PdfYomitokuMiniPageInfo
{
    public int? MainBodyPageNo;
    public int? MainBodyPageTotal;
    public int? PrefacePageNo;
    public int? PrefacePageTotal;
    public int PhysicalPageNo;
    public int PhysicalPageTotal;
    public bool? Ok;
}

// PDF OCR ライブラリ
public class PdfYomitokuLib
{
    readonly string YomiTokuPythonBaseDir;

    public PdfYomitokuLib(string yomiTokuPythonBaseDir)
    {
        this.YomiTokuPythonBaseDir = yomiTokuPythonBaseDir;
    }

    public async Task PerformOcrDirAsync(string srcPdfDirPath, string dstDirPath, string? ignorePathStr = null, CancellationToken cancel = default)
    {
        srcPdfDirPath = PP.RemoveLastSeparatorChar(srcPdfDirPath);
        dstDirPath = PP.RemoveLastSeparatorChar(dstDirPath);

        var srcPdfFiles = (await Lfs.EnumDirectoryAsync(srcPdfDirPath, true, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(".pdf")).OrderBy(x => x.FullPath, StrCmpi).ToList();

        if (ignorePathStr._IsFilled())
        {
            srcPdfFiles = srcPdfFiles.Where(x => PP.GetDirectoryName(x.FullPath)._InStri(ignorePathStr) == false).ToList();
        }

        int index = 0;

        foreach (var srcPdfFile in srcPdfFiles)
        {
            index++;

            string relativeFileNameWithoutExt = PP.GetPathWithoutExtension(PP.GetRelativeFileName(srcPdfFile.FullPath, srcPdfDirPath));

            List<PdfYomitokuOptions> optList = new();

            PdfYomitokuOptions baseOptions = new PdfYomitokuOptions
            {
                //Dpi = 100,
                //LiteMode = true, // 軽量化テスト用
            };

            {
                PdfYomitokuOptions o;

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.Pdf;
                optList.Add(o);

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.Html;
                optList.Add(o);

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.Html;
                o.Combine = false;
                optList.Add(o);

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.MD;
                optList.Add(o);

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.MD;
                o.Combine = false;
                optList.Add(o);

                o = baseOptions._CloneDeep();
                o.Format = PdfYomitokuFormats.JSON;
                optList.Add(o);
            }

            foreach (var opt in optList)
            {
                try
                {
                    string tagstr = opt.Format.ToString().ToLower();

                    if (opt.Format == PdfYomitokuFormats.Html || opt.Format == PdfYomitokuFormats.MD)
                    {
                        tagstr = tagstr + "_" + (opt.Combine ? "combined" : "paged");
                    }
                    else if (opt.Format == PdfYomitokuFormats.Pdf)
                    {
                        tagstr = "pdf_ocred";
                    }

                    // 宛先ルートディレクトリ
                    string dstRootDir = PP.Combine(dstDirPath, tagstr);

                    // 宛先ファイル名
                    string destFn = relativeFileNameWithoutExt + " [ " + tagstr.ToUpperInvariant() + " ]." + opt.Format.ToString().ToLowerInvariant();
                    string dstFilePath = PP.Combine(dstRootDir, destFn);

                    if (await Lfs.IsFileExistsAsync(dstFilePath, cancel: cancel) == false)
                    {
                        // ファイルが存在しないので処理を実施
                        $"({index} / {srcPdfFiles.Count()}) Performing OCR from \"{srcPdfFile.FullPath}\" to \"{dstFilePath}\" ..."._Print();

                        string internalShortName = srcPdfFile.FullPath.ToUpperInvariant()._HashSHA1()._GetHexString().Substring(0, 24).ToLowerInvariant();

                        await PerformOcrAsync(srcPdfFile.FullPath, internalShortName, dstFilePath, opt, cancel: cancel);
                    }
                }
                catch (Exception ex)
                {
                    $"*** Error *** ({index} / {srcPdfFiles.Count()}) Performing OCR from \"{srcPdfFile.FullPath}\". Options: {opt._ObjectToJson(compact: true)}"._Print();
                    ex._Error();
                }
            }
        }
    }

    public async Task PerformOcrAsync(string srcPdfPath, string internalShortName, string dstFilePath, PdfYomitokuOptions options, CancellationToken cancel = default)
    {
        string formatTypeStr = options.Format.ToString().ToLowerInvariant();

        internalShortName = Str.MakeVerySafeAsciiOnlyNonSpaceFileName(internalShortName, false);

        if (options.Format != PdfYomitokuFormats.MD && options.Format != PdfYomitokuFormats.Html && options.Combine == false)
        {
            throw new CoresLibException("options.Format != PdfYomitokuFormats.MD && options.Combine == false");
        }

        await Lfs.DeleteFileIfExistsAsync(dstFilePath, cancel: cancel);

        // ソース PDF のメタデータの読み込み
        var srcPdfMetaData = await Pdf2Txt.GetDocInfoFromPdfFileAsync(srcPdfPath, cancel: cancel);

        DateTimeOffset destFileTimeStamp = srcPdfMetaData.CreateDt;
        if (srcPdfMetaData.ModifyDt._IsZeroDateTimeForFileSystem() == false && destFileTimeStamp > srcPdfMetaData.ModifyDt) destFileTimeStamp = srcPdfMetaData.ModifyDt;
        if (destFileTimeStamp._IsZeroDateTimeForFileSystem()) destFileTimeStamp = new DateTime(1980, 1, 1);

        var destFileTimeStampMetaData = new FileMetadata(destFileTimeStamp);

        // 一時ディレクトリ作成 (すでに存在している場合は削除)
        string ocrTmpDirPath = PP.Combine(Env.MyLocalTempDir, "ocr");

        if (await Lfs.IsDirectoryExistsAsync(ocrTmpDirPath, cancel: cancel))
        {
            await Lfs.DeleteDirectoryAsync(ocrTmpDirPath, true, cancel: cancel);
        }

        await Lfs.CreateDirectoryAsync(ocrTmpDirPath, cancel: cancel);

        var now = DtNow;

        // ソース PDF ファイルをコピー
        string tmpTag = $"{now._ToYymmddStr()}_{now._ToHhmmssStr()}_{internalShortName.ToLowerInvariant()}";

        string ocrTmpSrcPdfPath = PP.Combine(ocrTmpDirPath, $"{tmpTag}.pdf");

        await Lfs.CopyFileAsync(srcPdfPath, ocrTmpSrcPdfPath);

        // 宛先ディレクトリを作成
        string ocrTmpDstDirPath = PP.Combine(ocrTmpDirPath, "ocrdst_" + formatTypeStr);
        await Lfs.CreateDirectoryAsync(ocrTmpDstDirPath, cancel: cancel);

        // コマンドプロンプト設計
        List<string> cmdList = new();

        cmdList.Add(ocrTmpSrcPdfPath._EnsureQuotation());

        cmdList.Add($"-f {formatTypeStr}");

        cmdList.Add("-o " + ocrTmpDstDirPath._EnsureQuotation());

        cmdList.Add("-d " + options.Device);

        cmdList.Add("--encoding " + options.Encoding);

        cmdList.Add("--dpi " + options.Dpi.ToString());

        if (options.IgnoreLineBreak)
        {
            cmdList.Add("--ignore_line_break");
        }

        if (options.Combine)
        {
            cmdList.Add("--combine");
        }

        if (options.OutputFigures)
        {
            cmdList.Add("--figure");

            if (options.OutputFigureLetters)
            {
                cmdList.Add("--figure_letter");
            }
        }

        if (options.IgnoreHeaderFooter)
        {
            cmdList.Add("--ignore_meta");
        }

        if (options.LiteMode)
        {
            cmdList.Add("--lite");
        }

        string cmdLine = "yomitoku " + cmdList._Combine(" ");

        await RunVEnvPythonCommandsAsync(cmdLine, options.TimeoutSecs * 1000, printTag: internalShortName, cancel: cancel);

        if (options.Format == PdfYomitokuFormats.Pdf)
        {
            // PDF の場合の追加処理
            string ocrDstGeneratedPdfPath = (await Lfs.EnumDirectoryAsync(ocrTmpDstDirPath, false, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(".pdf")).Single().FullPath;

            await SuperBookExternalTools.ImageMagick.ApplyDocInfoToPdfFileAsync(ocrDstGeneratedPdfPath, srcPdfMetaData, cancel: cancel);

            // 結果 PDF をユーザーが希望するパスにコピー
            await Lfs.EnsureCreateDirectoryForFileAsync(dstFilePath, cancel: cancel);

            await Lfs.CopyFileAsync(ocrDstGeneratedPdfPath, dstFilePath);

            var fileMeta = new FileMetadata(destFileTimeStamp);

            await Lfs.SetFileMetadataAsync(dstFilePath, destFileTimeStampMetaData, cancel: cancel);
        }
        else
        {
            // PDF 以外の場合の追加処理
            var tmpDestFiles = (await Lfs.EnumDirectoryAsync(ocrTmpDstDirPath, false, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(formatTypeStr)).OrderBy(x => x.FullPath, StrCmpi);

            string figuresDirName = ".figures_" + internalShortName.ToLowerInvariant() + "_" + formatTypeStr;

            foreach (var tmpDestFile in tmpDestFiles)
            {
                var body = await Lfs.ReadStringFromFileAsync(tmpDestFile.FullPath, cancel: cancel);

                body = body._ReplaceStr(
                    $"<img src=\"figures/",
                    $"<img src=\".figures/{figuresDirName}/",
                    true);

                await Lfs.WriteStringToFileAsync(tmpDestFile.FullPath, body, writeBom: true, cancel: cancel);
            }

            string figuresDirPathOld = PP.Combine(ocrTmpDstDirPath, "figures");
            string figuresDirPathNew = PP.Combine(ocrTmpDstDirPath, ".figures", figuresDirName);

            if (await Lfs.IsDirectoryExistsAsync(figuresDirPathOld))
            {
                string parentDir = PP.GetDirectoryName(figuresDirPathNew);
                await Lfs.CreateDirectoryAsync(parentDir);

                await Lfs.MoveDirectoryAsync(figuresDirPathOld, figuresDirPathNew, cancel: cancel);
            }

            string destTmpFileNameFinal;

            if (options.Combine)
            {
                destTmpFileNameFinal = tmpDestFiles.Single().FullPath;
            }
            else
            {
                destTmpFileNameFinal = PP.Combine(ocrTmpDstDirPath, "final." + formatTypeStr);

                Dictionary<int, (string FullPath, int LogicalPageNumber, Ref<bool> IsMainBody)> dict = new();

                foreach (var mdPage in tmpDestFiles)
                {
                    // ファイル名からページ番号を推定
                    string fn = mdPage.Name;
                    string tag = "ocr_" + tmpTag + "_p";

                    if (fn.StartsWith(tag, StrCmpi))
                    {
                        string tmp1 = fn.Substring(tag.Length);
                        int a1 = tmp1.IndexOf(".");
                        if (a1 != -1)
                        {
                            string tmp2 = tmp1.Substring(0, a1);

                            int physicalPageNumber = tmp2._ToInt();

                            if (physicalPageNumber >= 1)
                            {
                                int logicalPageNumber = physicalPageNumber;
                                bool isMainBody = false;

                                if (srcPdfMetaData.PhysicalPageStart.HasValue && srcPdfMetaData.LogicalPageStart.HasValue)
                                {
                                    if (physicalPageNumber >= srcPdfMetaData.PhysicalPageStart.Value)
                                    {
                                        logicalPageNumber = physicalPageNumber - srcPdfMetaData.PhysicalPageStart.Value + srcPdfMetaData.LogicalPageStart.Value;
                                        isMainBody = true;
                                    }
                                }

                                dict.Add(physicalPageNumber, (mdPage.FullPath, logicalPageNumber, isMainBody));
                            }
                        }
                    }
                }

                var pagesList = dict.OrderBy(x => x.Key).ToList();

                if (pagesList.Any(x => x.Value.IsMainBody) == false)
                {
                    // physical / logical マッピングがない PDF の場合
                    foreach (var page in pagesList)
                    {
                        page.Value.IsMainBody.Set(true);
                    }
                }

                // 結合
                StringWriter w = new();

                int mainBodyPagesTotal = pagesList.Select(x => x.Value).Where(x => x.IsMainBody).Select(x => x.LogicalPageNumber).DefaultIfEmpty(0).Max();
                int prefacePagesTotal = pagesList.Select(x => x.Value).Where(x => x.IsMainBody == false).Select(x => x.LogicalPageNumber).DefaultIfEmpty(0).Max();
                int physicalPagesTotal = pagesList.Select(x => x.Key).DefaultIfEmpty(0).Max();

                foreach (var page in pagesList)
                {
                    string body = await Lfs.ReadStringFromFileAsync(page.Value.FullPath, cancel: cancel);

                    PdfYomitokuMiniPageInfo info = new();

                    info.Ok = true;

                    info.PhysicalPageNo = page.Key;
                    info.PhysicalPageTotal = physicalPagesTotal;

                    if (page.Value.IsMainBody)
                    {
                        info.MainBodyPageNo = page.Value.LogicalPageNumber;
                        info.MainBodyPageTotal = mainBodyPagesTotal;
                    }
                    else
                    {
                        info.PrefacePageNo = page.Value.LogicalPageNumber;
                        info.PrefacePageTotal = prefacePagesTotal;
                    }


                    if (options.Format == PdfYomitokuFormats.MD)
                    {
                        w.WriteLine();
                        w.WriteLine("*****");
                        w.WriteLine();
                        w.WriteLine($"--- *ScanPageInfo: {info._ObjectToJson(compact: true).Replace(",", ", ").Replace(":", ": ").Replace("{", "{ ").Replace("}", " }")}* ---");
                        w.WriteLine();
                    }
                    else
                    {
                        w.WriteLine();
                        w.WriteLine("<HR>");
                        w.WriteLine();
                        w.WriteLine($"<p><i>--- ScanPageInfo: {info._ObjectToJson(compact: true).Replace(",", ", ").Replace(":", ": ").Replace("{", "{ ").Replace("}", " }")} ---</i></p>");
                        w.WriteLine();
                    }

                    w.WriteLine(body);
                }

                w.WriteLine();
                w.WriteLine();

                await Lfs.WriteStringToFileAsync(destTmpFileNameFinal, w.ToString(), writeBom: true, cancel: cancel);
            }

            // 結果のコピー
            await Lfs.EnsureCreateDirectoryForFileAsync(dstFilePath, cancel: cancel);

            await Lfs.CopyFileAsync(destTmpFileNameFinal, dstFilePath);

            var fileMeta = new FileMetadata(destFileTimeStamp);

            await Lfs.SetFileMetadataAsync(dstFilePath, destFileTimeStampMetaData, cancel: cancel);

            if (await Lfs.IsDirectoryExistsAsync(figuresDirPathNew, cancel: cancel))
            {
                string dstImgDirPath = PP.Combine(PP.GetDirectoryName(dstFilePath), ".figures", PP.GetFileName(figuresDirPathNew));

                await Lfs.CopyDirAsync(figuresDirPathNew, dstImgDirPath, cancel: cancel);

                foreach (var imgFile in (await Lfs.EnumDirectoryAsync(dstImgDirPath, cancel: cancel)).Where(x => x.IsFile))
                {
                    await Lfs.SetFileMetadataAsync(imgFile.FullPath, destFileTimeStampMetaData, cancel: cancel);
                }

                await Lfs.SetDirectoryMetadataAsync(dstImgDirPath, destFileTimeStampMetaData, cancel: cancel);

                string parentDir = PP.GetDirectoryName(dstImgDirPath);

                // ".figures" を隠しディレクトリにする
                try
                {
                    var meta = await Lfs.GetDirectoryMetadataAsync(parentDir, cancel: cancel);
                    if (meta.Attributes != null && meta.Attributes.Bit(FileAttributes.Hidden) == false)
                    {
                        FileMetadata meta2 = new FileMetadata(attributes: meta.Attributes.BitAdd(FileAttributes.Hidden));

                        await Lfs.SetDirectoryMetadataAsync(parentDir, meta2, cancel);
                    }
                }
                catch { }
            }
        }
    }

    async Task<EasyExecResult> RunVEnvPythonCommandsAsync(string commandLines,
        int timeout = Timeout.Infinite, bool throwOnErrorExitCode = true, string printTag = "",
        int easyOutputMaxSize = 0,
        Encoding? inputEncoding = null, Encoding? outputEncoding = null, Encoding? errorEncoding = null,
        CancellationToken cancel = default)
    {
        return await RunBatchCommandsDirectAsync(
            BuildLines("source ./venv/bin/activate",
            commandLines),
            timeout,
            throwOnErrorExitCode,
            printTag,
            easyOutputMaxSize,
            inputEncoding,
            outputEncoding,
            errorEncoding,
            cancel);
    }

    async Task<EasyExecResult> RunBatchCommandsDirectAsync(string commandLines,
        int timeout = Timeout.Infinite, bool throwOnErrorExitCode = true, string printTag = "",
        int easyOutputMaxSize = 0,
        Encoding? inputEncoding = null, Encoding? outputEncoding = null, Encoding? errorEncoding = null,
        CancellationToken cancel = default)
    {
        if (easyOutputMaxSize <= 0) easyOutputMaxSize = CoresConfig.DefaultAiUtilSettings.DefaultMaxStdOutBufferSize;

        string win32cmd = "/bin/bash";

        commandLines = BuildLines(commandLines, "exit");

        string tmp1 = "";
        if (printTag._IsFilled())
        {
            tmp1 += ": " + printTag.Trim();
        }
        string printTagMain = $"[YomiToku{tmp1}]";

        EasyExecResult ret = await EasyExec.ExecAsync(win32cmd, "", this.YomiTokuPythonBaseDir,
            easyInputStr: commandLines,
            flags: ExecFlags.Default | ExecFlags.EasyPrintRealtimeStdOut | ExecFlags.EasyPrintRealtimeStdErr,
            timeout: timeout, cancel: cancel, throwOnErrorExitCode: true,
            easyOutputMaxSize: easyOutputMaxSize,
            printTag: printTagMain,
            inputEncoding: inputEncoding, outputEncoding: outputEncoding, errorEncoding: errorEncoding);

        return ret;
    }
}

public enum DrawTextHorizonalAlign
{
    Left = 0,
    Right,
    Center,
}

public enum DrawTextVerticalAlign
{
    Top = 0,
    Bottom,
    Center,
}



public static class DnImageSharpHelper
{
    /// <summary>
    /// targetImage に対し、rect が示す領域の「内側」に収まる長方形枠線を描画する。
    /// </summary>
    /// <param name="targetImage">処理対象の <see cref="SixLabors.ImageSharp.Image{SixLabors.ImageSharp.PixelFormats.Rgb24}"/></param>
    /// <param name="rect">描画領域（外枠）</param>
    /// <param name="lineWidth">線の太さ（px 単位）</param>
    /// <param name="color">線の色（RGB 24bit）</param>
    /// <exception cref="ArgumentNullException">targetImage が null</exception>
    /// <exception cref="ArgumentOutOfRangeException">lineWidth が 0 以下</exception>
    /// <exception cref="ArgumentException">rect の幅または高さが 0 以下</exception>
    public static void DrawRect(
        this Image<Rgb24> targetImage,
        SixLabors.ImageSharp.Rectangle rect,
        double lineWidth,
        SixLabors.ImageSharp.Color color)
    {
        if (targetImage is null)
            throw new ArgumentNullException(nameof(targetImage));

        if (lineWidth <= 0)
            throw new ArgumentOutOfRangeException(nameof(lineWidth), "線幅は 0 より大きい必要があります。");

        if (rect.Width <= 0 || rect.Height <= 0)
            throw new ArgumentException("rect の幅および高さは正でなければなりません。", nameof(rect));

        // ───────────────────────────────────────────────────────────
        // 線がはみ出さないよう、path を線幅の半分だけ内側へオフセットする
        // ───────────────────────────────────────────────────────────
        float offset = (float)(lineWidth / 2.0);

        // 途中で負値にならないようにチェック
        float innerWidth = rect.Width - (float)lineWidth;
        float innerHeight = rect.Height - (float)lineWidth;
        if (innerWidth <= 0 || innerHeight <= 0)
        {
            // 枠線より矩形が小さい（あるいは同等）場合は描画しない
            return;
        }

        SixLabors.ImageSharp.RectangleF innerRect =
            new(rect.X + offset, rect.Y + offset, innerWidth, innerHeight);

        // ペンを生成（単色）
        var pen = SixLabors.ImageSharp.Drawing.Processing.Pens.Solid(color, (float)lineWidth);

        // 画像をミューテートして描画
        targetImage.Mutate(ctx =>
        {
            // ctx.Draw は IPath / RectangleF などを受け取る拡張メソッド
            ctx.Draw(pen, innerRect);
        });
    }





    public static void DrawSingleLineText(
        this SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgb24> image,
        string text,
        int x,
        int y,
        DrawTextHorizonalAlign horizonalAlign,
        DrawTextVerticalAlign verticalAlign,
        int height,
        string fontName,
        bool bold,
        SixLabors.ImageSharp.Color foreColor,
        SixLabors.ImageSharp.Color? backColor = null)
    {
        if (image is null) throw new ArgumentNullException(nameof(image));
        if (string.IsNullOrEmpty(text)) return;

        text = text.Replace("\r", string.Empty).Replace("\n", string.Empty);

        // フォント取得
        if (!SixLabors.Fonts.SystemFonts.TryGet(fontName, out SixLabors.Fonts.FontFamily family))
            throw new ArgumentException($"フォント '{fontName}' が見つかりません。", nameof(fontName));

        SixLabors.Fonts.FontStyle style =
            bold ? SixLabors.Fonts.FontStyle.Bold : SixLabors.Fonts.FontStyle.Regular;

        // ── 高さ指定 → フォントサイズ決定 ──
        float provisionalSize = height;
        SixLabors.Fonts.Font provisionalFont =
            new SixLabors.Fonts.Font(family, provisionalSize, style);

        SixLabors.Fonts.TextOptions measureOpt =
            new SixLabors.Fonts.TextOptions(provisionalFont);

        // 旧 Measure → 新 MeasureBounds
        SixLabors.Fonts.FontRectangle measured =
            SixLabors.Fonts.TextMeasurer.MeasureBounds(text, measureOpt);

        float measuredHeight = measured.Height > 0.01f ? measured.Height : 1f;
        float scale = height / measuredHeight;

        SixLabors.Fonts.Font font =
            new SixLabors.Fonts.Font(family, provisionalSize * scale, style);

        // 最終サイズ
        SixLabors.Fonts.TextOptions finalOpt =
            new SixLabors.Fonts.TextOptions(font);

        SixLabors.Fonts.FontRectangle finalRect =
            SixLabors.Fonts.TextMeasurer.MeasureBounds(text, finalOpt);

        float textWidth = finalRect.Width;
        float textHeight = finalRect.Height;

        // ── 描画原点計算 ──
        float drawX = x;
        float drawY = y;

        switch (horizonalAlign)
        {
            case DrawTextHorizonalAlign.Right: drawX -= textWidth; break;
            case DrawTextHorizonalAlign.Center: drawX -= textWidth / 2f; break;
        }
        switch (verticalAlign)
        {
            case DrawTextVerticalAlign.Bottom: drawY -= textHeight; break;
            case DrawTextVerticalAlign.Center: drawY -= textHeight / 2f; break;
        }

        // ── 描画 ──
        image.Mutate(ctx =>
        {
            if (backColor.HasValue)
            {
                ctx.Fill(backColor.Value,
                    new SixLabors.ImageSharp.RectangleF(drawX, drawY, textWidth, textHeight));
            }

            ctx.DrawText(text, font, foreColor,
                new SixLabors.ImageSharp.PointF(drawX, drawY));
        });
    }



    /// <summary>
    /// <para>rectList 全体を同一平面に重ねたとき、  
    /// 最も多くの矩形が重なっている領域を表す <see cref="SixLabors.ImageSharp.Rectangle"/> を返します。</para>
    /// <para>最大重なり枚数の領域が複数ある場合は、  
    /// 全体のバウンディングボックス中心に最も近い領域（セル）を選択します。</para>
    /// <para>計算のしくみ：  
    /// 各矩形の Left/Right, Top/Bottom を軸に区切ることで平面を格子状セルに分割し、  
    /// セルごとに被覆枚数を数えて評価します。  
    /// 枚数が最大となるセルの Left‐Right, Top‐Bottom がそのまま返値の矩形座標になります。</para>
    /// </summary>
    /// <param name="rectList">重ね合わせ対象となる矩形列。</param>
    /// <returns>最厚領域を示す <see cref="SixLabors.ImageSharp.Rectangle"/>。</returns>
    /// <exception cref="ArgumentNullException"><paramref name="rectList"/> が null。</exception>
    /// <exception cref="ArgumentException"><paramref name="rectList"/> が空。</exception>
    /// <exception cref="InvalidOperationException">有効な重なり領域が見つからない場合。</exception>
    public static SixLabors.ImageSharp.Rectangle CalcMostOverlapRect(
        this IEnumerable<SixLabors.ImageSharp.Rectangle> rectList)
    {
        if (rectList is null)
        {
            throw new ArgumentNullException(nameof(rectList));
        }

        var list = rectList.ToList();
        if (list.Count == 0)
        {
            throw new ArgumentException("rectList must contain at least one rectangle.", nameof(rectList));
        }

        // --- 1. 端点集合を作成 ------------------------------------------------------------
        var xEdgeSet = new HashSet<int>();
        var yEdgeSet = new HashSet<int>();

        foreach (SixLabors.ImageSharp.Rectangle r in list)
        {
            xEdgeSet.Add(r.Left);
            xEdgeSet.Add(r.Right);
            yEdgeSet.Add(r.Top);
            yEdgeSet.Add(r.Bottom);
        }

        int[] xEdges = xEdgeSet.OrderBy(v => v).ToArray();
        int[] yEdges = yEdgeSet.OrderBy(v => v).ToArray();

        // --- 2. 全体バウンディングボックス中心座標（比較用） ------------------------------
        int globalLeft = list.Min(r => r.Left);
        int globalTop = list.Min(r => r.Top);
        int globalRight = list.Max(r => r.Right);
        int globalBottom = list.Max(r => r.Bottom);

        double globalCenterX = (globalLeft + globalRight) / 2.0;
        double globalCenterY = (globalTop + globalBottom) / 2.0;

        // --- 3. セル走査 ---------------------------------------------------------------
        int bestCount = -1;
        double bestDist2 = double.MaxValue;
        SixLabors.ImageSharp.Rectangle bestRect = default;

        for (int xi = 0; xi < xEdges.Length - 1; xi++)
        {
            int left = xEdges[xi];
            int right = xEdges[xi + 1];
            if (left == right) continue;              // 幅ゼロセルは無視

            for (int yi = 0; yi < yEdges.Length - 1; yi++)
            {
                int top = yEdges[yi];
                int bottom = yEdges[yi + 1];
                if (top == bottom) continue;          // 高さゼロセルは無視

                // ■ このセルを完全に覆う矩形枚数を数える
                int count = 0;
                foreach (SixLabors.ImageSharp.Rectangle r in list)
                {
                    if (r.Left <= left && r.Right >= right &&
                        r.Top <= top && r.Bottom >= bottom)
                    {
                        count++;
                    }
                }
                if (count == 0) continue;

                // ■ 更新条件：より厚い  or  同厚で中心に近い
                double cellCx = (left + right) / 2.0;
                double cellCy = (top + bottom) / 2.0;
                double dist2 = (cellCx - globalCenterX) * (cellCx - globalCenterX) +
                                 (cellCy - globalCenterY) * (cellCy - globalCenterY);

                if (count > bestCount ||
                   (count == bestCount && dist2 < bestDist2))
                {
                    bestCount = count;
                    bestDist2 = dist2;
                    bestRect = new SixLabors.ImageSharp.Rectangle(
                                    left, top,
                                    right - left,
                                    bottom - top);
                }
            }
        }

        if (bestCount <= 0)
        {
            throw new InvalidOperationException("No overlapping area found.");
        }

        return bestRect;
    }

    /// <summary>
    /// 2 点間のユークリッド距離を計算します。
    /// </summary>
    /// <returns>2 点間の距離 (double)。</returns>
    public static double CalcDistance(this SixLabors.ImageSharp.Point p1, SixLabors.ImageSharp.Point p2)
    {
        // System.Drawing.Point は整数座標を保持するため、オーバーフローを避ける目的で long に昇格
        long dx = (long)p2.X - p1.X;
        long dy = (long)p2.Y - p1.Y;

        return Math.Sqrt(dx * dx + dy * dy);
    }

    /// <summary>
    /// 指定された <see cref="SixLabors.ImageSharp.Rectangle"/> の中心点を計算して返します。  
    /// 画像座標系（左上原点・右方向が X 増、下方向が Y 増）を想定し、
    /// 幅・高さが偶数のときは左上寄りの画素を採用します。
    /// </summary>
    /// <param name="rect">中心を求めたい矩形。</param>
    /// <returns>
    /// 矩形の中心を表す <see cref="SixLabors.ImageSharp.Point"/>。
    /// </returns>
    public static SixLabors.ImageSharp.Point GetRectCenterPoint(this SixLabors.ImageSharp.Rectangle rect)
    {
        // (X + Width / 2, Y + Height / 2) を整数座標で求める。
        int centerX = rect.X + (rect.Width >> 1);   // >> 1 は /2 と同義（整数演算）
        int centerY = rect.Y + (rect.Height >> 1);

        return new SixLabors.ImageSharp.Point(centerX, centerY);
    }


    /// <summary>
    /// <paramref name="parentRect"/> が <paramref name="childRect"/> を完全に包含しているかどうかを判定します。
    /// 辺が一致している（接している）場合も「包含」と見なします。
    /// </summary>
    /// <param name="parentRect">包含側の長方形。</param>
    /// <param name="childRect">内包される側の長方形。</param>
    /// <returns>包含していれば <c>true</c>、そうでなければ <c>false</c>。</returns>
    public static bool RectContainsOrExactSameToRect(
        this SixLabors.ImageSharp.Rectangle parentRect,
        SixLabors.ImageSharp.Rectangle childRect)
    {
        // Inclusive containment check
        return childRect.X >= parentRect.X &&
               childRect.Y >= parentRect.Y &&
               childRect.Right <= parentRect.Right &&
               childRect.Bottom <= parentRect.Bottom;
    }

    /// <summary>
    /// <para>rectList に含まれる <see cref="SixLabors.ImageSharp.Rectangle"/> すべてを
    /// 内包する最小の <see cref="SixLabors.ImageSharp.Rectangle"/> を返します。</para>
    /// <para>座標系は ImageSharp 準拠（左上原点、右方向が +X、下方向が +Y）です。</para>
    /// </summary>
    /// <param name="rectList">
    /// 包含対象となる矩形コレクション。
    /// null や空コレクションの場合は例外を送出します。
    /// </param>
    /// <returns>
    /// 入力矩形をすべて包含する最小の矩形。
    /// </returns>
    /// <exception cref="ArgumentNullException">
    /// rectList が null。
    /// </exception>
    /// <exception cref="ArgumentException">
    /// rectList が空。
    /// </exception>
    public static SixLabors.ImageSharp.Rectangle GetBoundingBoxFromRectangles(
        this IEnumerable<SixLabors.ImageSharp.Rectangle> rectList)
    {
        if (rectList is null)
        {
            throw new ArgumentNullException(nameof(rectList));
        }

        // イテレーションを開始する前に「値が入ったかどうか」を判定するフラグ
        var hasValue = false;

        // 包含領域を表す境界値
        int left = 0;
        int top = 0;
        int right = 0;
        int bottom = 0;

        foreach (SixLabors.ImageSharp.Rectangle rect in rectList)
        {
            if (!hasValue)
            {
                // 初回のみ現在の矩形で初期化
                left = rect.Left;
                top = rect.Top;
                right = rect.Right;
                bottom = rect.Bottom;
                hasValue = true;
                continue;
            }

            // より外側に広がる座標があれば更新
            if (rect.Left < left) left = rect.Left;
            if (rect.Top < top) top = rect.Top;
            if (rect.Right > right) right = rect.Right;
            if (rect.Bottom > bottom) bottom = rect.Bottom;
        }

        if (!hasValue)
        {
            throw new ArgumentException("rectList must contain at least one rectangle.", nameof(rectList));
        }

        // SixLabors.ImageSharp.Rectangle には LTRB で生成できるファクトリがある
        return SixLabors.ImageSharp.Rectangle.FromLTRB(left, top, right, bottom);
    }

    /// <summary>
    /// 基準矩形 <paramref name="baseRect"/> を原点と見なし、
    /// 相対矩形 <paramref name="relativeRect"/> を絶対座標へ変換して返すユーティリティ関数。
    /// <para>▪ <paramref name="clip"/> が <c>true</c> の場合は、基準矩形をはみ出す部分をクリッピングする。</para>
    /// <para>▪ <paramref name="clip"/> が <c>false</c> の場合は領域チェック・クリッピングを一切行わず、
    ///   単純に加算した結果をそのまま返す（はみ出し許容）。</para>
    /// <para>▪ <paramref name="clip"/> が <c>true</c> で、まったく重なり合いがない場合は <c>null</c> を返す。</para>
    /// <para>▪ 例外は投げない。</para>
    /// </summary>
    /// <param name="baseRect">基準 (絶対) 座標の矩形</param>
    /// <param name="relativeRect">基準矩形左上を (0,0) とした相対座標の矩形</param>
    /// <param name="clip">
    /// true  : はみ出す部分を基準矩形でクリッピング。重なりゼロなら <c>null</c>。<br/>
    /// false : 領域チェックせず単純加算。はみ出し許容で常に矩形を返す。
    /// </param>
    /// <returns>
    /// ・<paramref name="clip"/> が true  : クリッピング後の矩形、または重なり無しなら <c>null</c>。<br/>
    /// ・<paramref name="clip"/> が false : 単純加算した矩形（必ず非 null）。
    /// </returns>
    public static Rectangle? GetAbsoluteRectFromRelativeChildRect(
        this Rectangle baseRect,
        Rectangle relativeRect,
        bool clip = false)
    {
        // --- 相対 → 絶対座標へ変換（サイズはそのまま）---
        int absLeft = baseRect.X + relativeRect.X;
        int absTop = baseRect.Y + relativeRect.Y;

        // clip=false の場合は領域チェックを行わず、そのまま返す
        if (!clip)
        {
            return new Rectangle(absLeft, absTop, relativeRect.Width, relativeRect.Height);
        }

        // --- クリッピング処理 (clip==true) ------------------
        int absRight = absLeft + relativeRect.Width;
        int absBottom = absTop + relativeRect.Height;

        int baseRight = baseRect.X + baseRect.Width;
        int baseBottom = baseRect.Y + baseRect.Height;

        // 相互交差領域を計算
        int clipLeft = Math.Max(absLeft, baseRect.X);
        int clipTop = Math.Max(absTop, baseRect.Y);
        int clipRight = Math.Min(absRight, baseRight);
        int clipBottom = Math.Min(absBottom, baseBottom);

        int clipWidth = clipRight - clipLeft;
        int clipHeight = clipBottom - clipTop;

        // 重なりが無い場合
        if (clipWidth <= 0 || clipHeight <= 0)
        {
            return null;
        }

        // クリップ後の矩形を返す
        return new Rectangle(clipLeft, clipTop, clipWidth, clipHeight);
    }


    #region ── ランダム色ジェネレーター ─────────────────────────────────────

    private static readonly Random _rng = new();

    /// <summary>
    /// 白背景・黒文字上でも読みやすく、かつカラフルで互いに識別しやすいランダム色を返す。
    /// <para>任意で <paramref name="avoidColor"/> を指定すると、その色に「似すぎる」ものを避ける。</para>
    /// </summary>
    /// <param name="avoidColor">
    ///   <see langword="null"/> の場合 : 制限なし（従来どおり）。<br/>
    ///   指定あり                 : ΔE(CIE76) が <c>MinDeltaE</c> 未満の候補をスキップ。
    /// </param>
    public static Color GenerateRandomGoodColor(Color? avoidColor = null)
    {
        const float minS = 0.60f;   // 彩度  60–100 %
        const float maxS = 1.00f;
        const float minL = 0.35f;   // 輝度  35–65 %
        const float maxL = 0.65f;
        const double MinDeltaE = 15.0;

        // avoidColor の Lab 値を一度だけ作成しておく
        (double L, double A, double B)? avoidLab = null;
        if (avoidColor.HasValue)
        {
            avoidLab = ToLab(avoidColor.Value);
        }

        while (true)
        {
            // --- HSL 空間でランダム生成 -----------------------------------
            float h = (float)_rng.NextDouble() * 360f;
            float s = minS + (float)_rng.NextDouble() * (maxS - minS);
            float l = minL + (float)_rng.NextDouble() * (maxL - minL);

            // HSL → RGB (0–1.0)
            Rgb rgb = ColorSpaceConverter.ToRgb(new Hsl(h, s, l));

            Color candidate = Color.FromRgb(
                (byte)(rgb.R * 255f),
                (byte)(rgb.G * 255f),
                (byte)(rgb.B * 255f));

            // --- ① 白・黒とのコントラストチェック ------------------------
            if (!IsReadableOnWhiteAndBlack(candidate))
                continue;

            // --- ② 指定色との類似度チェック ------------------------------
            if (avoidLab.HasValue)
            {
                var labC = ToLab(candidate);
                var labRef = avoidLab.Value;

                double deltaE = Math.Sqrt(
                    Math.Pow(labC.L - labRef.L, 2) +
                    Math.Pow(labC.A - labRef.A, 2) +
                    Math.Pow(labC.B - labRef.B, 2));

                if (deltaE < MinDeltaE)
                    continue;               // 似すぎ：NG
            }

            // --- すべての条件を満たしたので返す --------------------------
            return candidate;
        }
    }

    #endregion

    #region ── コントラスト・輝度計算ヘルパ ──────────────────────────────

    private static bool IsReadableOnWhiteAndBlack(Color c)
    {
        const double minContrast = 3.0; // WCAG “Large Text” 相当
        return ContrastRatio(c, Color.White) >= minContrast
            && ContrastRatio(c, Color.Black) >= minContrast;
    }

    private static double ContrastRatio(Color a, Color b)
    {
        double l1 = RelativeLuminance(a);
        double l2 = RelativeLuminance(b);
        double brighter = Math.Max(l1, l2);
        double darker = Math.Min(l1, l2);
        return (brighter + 0.05) / (darker + 0.05);
    }

    private static double RelativeLuminance(Color c)
    {
        Rgba32 rgba = c.ToPixel<Rgba32>();

        double Rs = rgba.R / 255.0;
        double Gs = rgba.G / 255.0;
        double Bs = rgba.B / 255.0;

        double R = Rs <= 0.03928 ? Rs / 12.92 : Math.Pow((Rs + 0.055) / 1.055, 2.4);
        double G = Gs <= 0.03928 ? Gs / 12.92 : Math.Pow((Gs + 0.055) / 1.055, 2.4);
        double B = Bs <= 0.03928 ? Bs / 12.92 : Math.Pow((Bs + 0.055) / 1.055, 2.4);

        return 0.2126 * R + 0.7152 * G + 0.0722 * B;
    }

    #endregion

    #region ── sRGB → CIE Lab 変換 ────────────────────────────────────────
    /// <summary>
    /// ImageSharp <see cref="Color"/> (sRGB) を CIE Lab 値 (D65) に変換する。
    /// </summary>
    private static (double L, double A, double B) ToLab(Color c)
    {
        // 1. 0-255 → 0-1 へ正規化し、sRGB → 線形 RGB へ
        Rgba32 p = c.ToPixel<Rgba32>();

        double sr = p.R / 255.0;
        double sg = p.G / 255.0;
        double sb = p.B / 255.0;

        double R = sr <= 0.04045 ? sr / 12.92 : Math.Pow((sr + 0.055) / 1.055, 2.4);
        double G = sg <= 0.04045 ? sg / 12.92 : Math.Pow((sg + 0.055) / 1.055, 2.4);
        double B = sb <= 0.04045 ? sb / 12.92 : Math.Pow((sb + 0.055) / 1.055, 2.4);

        // 2. 線形 RGB → CIE XYZ (D65)
        double X = 0.4124564 * R + 0.3575761 * G + 0.1804375 * B;
        double Y = 0.2126729 * R + 0.7151522 * G + 0.0721750 * B;
        double Z = 0.0193339 * R + 0.1191920 * G + 0.9503041 * B;

        // 3. XYZ → Lab
        const double Xn = 0.95047;   // D65 白色点
        const double Yn = 1.00000;
        const double Zn = 1.08883;

        static double F(double t) => t > 0.008856 ? Math.Pow(t, 1.0 / 3.0) : (7.787 * t) + (16.0 / 116.0);

        double fx = F(X / Xn);
        double fy = F(Y / Yn);
        double fz = F(Z / Zn);

        double L = (116.0 * fy) - 16.0;
        double A = 500.0 * (fx - fy);
        double Bc = 200.0 * (fy - fz);

        return (L, A, Bc);
    }
    #endregion

}

public static class SuperImgUtil
{

    /// <summary>
    /// srcFilePath で指定された画像を RGB24 で読み込み、
    /// srcColor のベタ塗り領域（連続ピクセルが srcColorPixelRenzokuCount 以上）を検出。
    /// その領域を横幅×extra ピクセル分だけ（上下左右に radius ピクセル）拡張した後、
    /// dstFilesList に記載された各 DstColor で置換し、各 DstPngFilePath へ 24-bit PNG として保存する。
    /// 検出＋拡張は 1 回だけ実行し、書き出しは最大 numCpu 並列で行う。
    /// </summary>
    public static async Task ReplaceBmpColorAsync(
        string srcFilePath,
        List<(string DstPngFilePath, Rgb24? DstColor, double Extra)> dstFilesList,
        Rgb24 srcColor,
        int srcColorPixelRenzokuCount,
        int numCpu)
    {
        // ---------- 引数チェック ----------
        if (string.IsNullOrWhiteSpace(srcFilePath))
            throw new ArgumentException(nameof(srcFilePath));
        if (!File.Exists(srcFilePath))
            throw new FileNotFoundException(srcFilePath);
        if (dstFilesList is null || dstFilesList.Count == 0)
            return;
        if (dstFilesList.Any(t => string.IsNullOrWhiteSpace(t.DstPngFilePath)))
            throw new ArgumentException($"空文字列の {nameof(dstFilesList)} 要素があります。");
        if (srcColorPixelRenzokuCount <= 0)
            throw new ArgumentOutOfRangeException(nameof(srcColorPixelRenzokuCount));
        if (numCpu <= 0)
            throw new ArgumentOutOfRangeException(nameof(numCpu));

        // ---------- 画像読み込み（α付きでも強制 RGB24）----------
        using Image<Rgb24> original =
            await Image.LoadAsync<Rgb24>(srcFilePath).ConfigureAwait(false);

        int width = original.Width;
        int height = original.Height;
        int pixelCount = width * height;

        // ---------- 1. ベタ塗り領域検出（1 回だけ） ----------
        bool[] replaceMask = new bool[pixelCount];   // 塗り替え対象
        bool[] visited = new bool[pixelCount];   // BFS 用
        System.Collections.Generic.Queue<int> bfsQ = new();
        System.Collections.Generic.List<int> cluster = new();
        int[] dx = { -1, 1, 0, 0 };
        int[] dy = { 0, 0, -1, 1 };

        original.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < height; y++)
            {
                System.Span<Rgb24> row = accessor.GetRowSpan(y);

                for (int x = 0; x < width; x++)
                {
                    int idx = y * width + x;
                    if (visited[idx] || !row[x].Equals(srcColor)) continue;

                    // --- 連結成分（4近傍）を BFS で収集 ---
                    bfsQ.Clear();
                    cluster.Clear();

                    bfsQ.Enqueue(idx);
                    visited[idx] = true;
                    cluster.Add(idx);

                    while (bfsQ.Count > 0)
                    {
                        int cur = bfsQ.Dequeue();
                        int cy = cur / width;
                        int cx = cur % width;

                        for (int k = 0; k < 4; k++)
                        {
                            int nx = cx + dx[k];
                            int ny = cy + dy[k];
                            if ((uint)nx >= (uint)width || (uint)ny >= (uint)height)
                                continue;

                            int nIdx = ny * width + nx;
                            if (visited[nIdx]) continue;
                            if (original[nx, ny].Equals(srcColor))
                            {
                                visited[nIdx] = true;
                                bfsQ.Enqueue(nIdx);
                                cluster.Add(nIdx);
                            }
                        }
                    }

                    if (cluster.Count >= srcColorPixelRenzokuCount)
                    {
                        foreach (int p in cluster)
                            replaceMask[p] = true;
                    }
                }
            }
        });

        // ---------- 3. 並列で各ファイルを書き出し ----------
        SixLabors.ImageSharp.Formats.Png.PngEncoder encoder =
            new() { ColorType = SixLabors.ImageSharp.Formats.Png.PngColorType.Rgb };

        await Parallel.ForEachAsync(
            dstFilesList,
            new ParallelOptions { MaxDegreeOfParallelism = numCpu },
            async (item, _) =>
            {
                bool[] replaceMask2;
                // ---------- 2. extra 指定分だけマスクを拡張 ----------
                int radius = (int)Math.Round(width * item.Extra);   // 上下左右に広げるピクセル数
                if (radius > 0)
                {
                    replaceMask2 = ExpandMask(replaceMask, width, height, radius);
                }
                else
                {
                    replaceMask2 = replaceMask._CloneDeep();
                }

                string outPath = item.DstPngFilePath;
                Rgb24? targetColor = item.DstColor;
                if (targetColor == null)
                {
                    //targetColor = CreateRandomColorfulColor(original);
                    targetColor = CreateRandomBlueColor();
                }

                // --- 画像クローン（書き換え用）---
                using Image<Rgb24> img = original.Clone();

                img.ProcessPixelRows(rowAccessor =>
                {
                    for (int y = 0; y < height; y++)
                    {
                        System.Span<Rgb24> row = rowAccessor.GetRowSpan(y);
                        int baseIdx = y * width;
                        for (int x = 0; x < width; x++)
                        {
                            if (replaceMask2[baseIdx + x])
                                row[x] = targetColor.Value;
                        }
                    }
                });

                // 出力先ディレクトリが無ければ作成
                string? dir = System.IO.Path.GetDirectoryName(outPath);
                if (!string.IsNullOrEmpty(dir) && !Directory.Exists(dir))
                    Directory.CreateDirectory(dir);

                await img.SaveAsync(outPath, encoder).ConfigureAwait(false);
            });
    }

    // ------------------------------------------------------------------
    // ★ private helper : bool マスクを radius 分だけ矩形膨張（高速積分画像実装）
    // ------------------------------------------------------------------
    private static bool[] ExpandMask(bool[] src, int width, int height, int radius)
    {
        int stride = width + 1;                         // 積分画像は (w+1)×(h+1)
        int[] integral = new int[stride * (height + 1)];

        // --- 1. 積分画像作成 ---
        for (int y = 1; y <= height; y++)
        {
            int rowSum = 0;
            int srcBase = (y - 1) * width;
            int dstBase = y * stride;
            int dstPrev = (y - 1) * stride;

            for (int x = 1; x <= width; x++)
            {
                rowSum += src[srcBase + (x - 1)] ? 1 : 0;
                integral[dstBase + x] = integral[dstPrev + x] + rowSum;
            }
        }

        // --- 2. 積分画像を用いて矩形膨張結果を生成 ---
        bool[] dst = new bool[src.Length];

        for (int y = 0; y < height; y++)
        {
            int y1 = Math.Max(0, y - radius);
            int y2 = Math.Min(height - 1, y + radius);

            int top = y1 * stride;
            int bottom = (y2 + 1) * stride;

            for (int x = 0; x < width; x++)
            {
                int x1 = Math.Max(0, x - radius);
                int x2 = Math.Min(width - 1, x + radius);

                int sum = integral[bottom + x2 + 1] - integral[top + x2 + 1]
                        - integral[bottom + x1] + integral[top + x1];

                dst[y * width + x] = sum > 0;          // 範囲内に 1 ピクセルでもあれば ON
            }
        }
        return dst;
    }


    /// <summary>
    /// ランダムな青系カラーを 1 色返す。
    /// - 青チャネルが支配的 (B &gt; R, G)
    /// - 相対輝度 Y は純青 (RGB 0,0,255) 以下
    /// - ただし真っ黒にはならず、肉眼で青と分かる (輝度下限 6)
    /// </summary>
    public static Rgb24 CreateRandomBlueColor()
    {
        const double blueLuminance = 0.0722 * 255; // ≒ 18.4 : 純青の輝度
        var rnd = Random.Shared;

        while (true)
        {
            // ─ HSL の範囲 ─────────────────────────────
            double h = 220 + rnd.NextDouble() * 40;   // Hue : 220°–260° (青→藍)
            double s = 0.7 + rnd.NextDouble() * 0.3; // Saturation : 0.7–1.0
            double l = 0.15 + rnd.NextDouble() * 0.35;// Lightness : 0.15–0.50 (≤0.5)

            // HSL → RGB (0–255)
            var (r, g, b) = HslToRgb(h, s, l);

            // ─ 条件チェック ───────────────────────────
            if (b > r && b > g)                      // 明らかに青
            {
                double y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                if (y <= blueLuminance && y >= 6)    // 明るさ: 青以下、かつ黒すぎない
                {
                    return new Rgb24(r, g, b);
                }
            }
            // 条件を満たさない場合は再試行
        }
    }

    // ───────────────────────────────────────────────
    // 内部: HSL → RGB 変換 (sRGB)  0–255 Byte で返す
    private static (byte r, byte g, byte b) HslToRgb(double h, double s, double l)
    {
        h %= 360.0;

        double c = (1 - Math.Abs(2 * l - 1)) * s;
        double hPrime = h / 60.0;
        double x = c * (1 - Math.Abs(hPrime % 2 - 1));

        double r1 = 0, g1 = 0, b1 = 0;
        if (hPrime < 1) { r1 = c; g1 = x; }
        else if (hPrime < 2) { r1 = x; g1 = c; }
        else if (hPrime < 3) { g1 = c; b1 = x; }
        else if (hPrime < 4) { g1 = x; b1 = c; }
        else if (hPrime < 5) { r1 = x; b1 = c; }
        else { r1 = c; b1 = x; }

        double m = l - c / 2.0;
        return (
            (byte)Math.Round((r1 + m) * 255.0),
            (byte)Math.Round((g1 + m) * 255.0),
            (byte)Math.Round((b1 + m) * 255.0)
        );
    }



    /// <summary>
    /// 「目立つ」「彩度高め」「やや濃い」色をランダム生成して返す。
    /// referenceImage が指定されている場合は，その画像上で線を引いたときに
    /// よく目立つよう，画像の代表色と十分にコントラストがある色を選ぶ。
    /// </summary>
    /// <param name="referenceImage">
    ///   目立たせたい対象画像（null 可）</param>
    public static SixLabors.ImageSharp.PixelFormats.Rgb24 CreateRandomColorfulColor(
        SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgb24>? referenceImage = null)
    {
        // --- 1. まず画像側の「代表色」を取得 -------------------------------
        (double L, double a, double b) refLab = (0, 0, 0);
        bool hasRef = referenceImage is not null;

        if (hasRef)
        {
            // 画像全体を走査すると重いので，ランダムサンプリングで代表色を推定する。
            const int sampleCount = 2_000;                         // 画素サンプル数上限
            System.Random rng = System.Random.Shared;
            int w = referenceImage!.Width;
            int h = referenceImage.Height;

            double sumL = 0, suma = 0, sumb = 0;
            int taken = 0;
            for (int i = 0; i < sampleCount; i++)
            {
                int x = rng.Next(w);
                int y = rng.Next(h);
                SixLabors.ImageSharp.PixelFormats.Rgb24 px = referenceImage[x, y];
                (double L, double a, double b) lab = RgbToLab(px);
                sumL += lab.L; suma += lab.a; sumb += lab.b;
                taken++;
            }
            refLab = (sumL / taken, suma / taken, sumb / taken);
        }

        // --- 2. 「鮮やかな色」の色相レンジ定義 ------------------------------
        (double s, double e)[] hueRanges =
        {
            (315, 345), // pink
            (350, 360), // red (upper)
            (  0,  15), // red (lower)
            (260, 290), // purple
            (200, 230), // blue
            (200, 250), // navy
            ( 90, 150), // green
            (160, 180), // tropical / turquoise
            (210, 230), // royal blue
            (195, 205), // LightSkyBlue
        };

        // --- 3. コントラスト条件を満たすまで乱択 -----------------------------
        const int MAX_TRIES = 100;
        const int MIN_BRIGHT = 20;
        const int MAX_BRIGHT = 80;
        const double MIN_DELTA_E = 40.0;  // 画像との Lab 距離の下限値

        System.Random rnd = System.Random.Shared;
        for (int t = 0; t < MAX_TRIES; t++)
        {
            // 3-1. 候補色を生成（従来ロジック）
            var (hs, he) = hueRanges[rnd.Next(hueRanges.Length)];
            double hue = hs + rnd.NextDouble() * (he - hs);
            double sat = 0.7 + rnd.NextDouble() * 0.3;           // 0.7-1.0
            double light = 0.18 + rnd.NextDouble() * 0.16;       // HSL L 0.18-0.34
            SixLabors.ImageSharp.PixelFormats.Rgb24 cand = HslToRgb(hue, sat, light);

            int ave = (cand.R + cand.G + cand.B) / 3;
            if (ave < MIN_BRIGHT || ave > MAX_BRIGHT) continue;  // 明度条件

            if (hasRef)
            {
                // 3-2. 画像平均 Lab との距離を測り，十分離れていなければリトライ
                (double L, double a, double b) candLab = RgbToLab(cand);
                double deltaE = Math.Sqrt(
                    Math.Pow(candLab.L - refLab.L, 2) +
                    Math.Pow(candLab.a - refLab.a, 2) +
                    Math.Pow(candLab.b - refLab.b, 2));

                if (deltaE < MIN_DELTA_E) continue;              // コントラスト不足
            }
            // 条件を満たした！
            return cand;
        }

        // 100 回試しても見つからなかった場合は，最後の候補を返す（まず起きない）
        return HslToRgb(0, 1, 0.25);


        // ---------- 以降ローカル関数群 ------------------------------------

        static SixLabors.ImageSharp.PixelFormats.Rgb24 HslToRgb(double h, double s, double l)
        {
            h = (h % 360) / 360.0;
            double r, g, b;

            if (s == 0)
            {
                r = g = b = l; // 無彩色
            }
            else
            {
                double q = l < 0.5 ? l * (1 + s) : l + s - l * s;
                double p = 2 * l - q;
                r = HueToRgb(p, q, h + 1.0 / 3.0);
                g = HueToRgb(p, q, h);
                b = HueToRgb(p, q, h - 1.0 / 3.0);
            }
            return new SixLabors.ImageSharp.PixelFormats.Rgb24(
                (byte)Math.Clamp(Math.Round(r * 255), 0, 255),
                (byte)Math.Clamp(Math.Round(g * 255), 0, 255),
                (byte)Math.Clamp(Math.Round(b * 255), 0, 255));

            static double HueToRgb(double p, double q, double t)
            {
                if (t < 0) t += 1;
                if (t > 1) t -= 1;
                if (t < 1.0 / 6.0) return p + (q - p) * 6 * t;
                if (t < 1.0 / 2.0) return q;
                if (t < 2.0 / 3.0) return p + (q - p) * (2.0 / 3.0 - t) * 6;
                return p;
            }
        }

        // sRGB → CIELAB 変換（CIE76 ΔE 用）
        static (double L, double a, double b) RgbToLab(SixLabors.ImageSharp.PixelFormats.Rgb24 rgb)
        {
            // 1. sRGB → 線形 RGB
            static double SrgbToLinear(byte c)
            {
                double v = c / 255.0;
                return v <= 0.04045 ? v / 12.92
                                    : Math.Pow((v + 0.055) / 1.055, 2.4);
            }
            double rl = SrgbToLinear(rgb.R);
            double gl = SrgbToLinear(rgb.G);
            double bl = SrgbToLinear(rgb.B);

            // 2. 線形 RGB → XYZ (D65)
            double X = (rl * 0.4124 + gl * 0.3576 + bl * 0.1805) * 100.0;
            double Y = (rl * 0.2126 + gl * 0.7152 + bl * 0.0722) * 100.0;
            double Z = (rl * 0.0193 + gl * 0.1192 + bl * 0.9505) * 100.0;

            // 3. XYZ → LAB
            static double F(double t)
            {
                const double δ = 6.0 / 29.0;
                return t > Math.Pow(δ, 3) ? Math.Cbrt(t)
                                           : (t / (3 * δ * δ) + 4.0 / 29.0);
            }
            // D65 標準光
            const double Xn = 95.047, Yn = 100.0, Zn = 108.883;

            double fx = F(X / Xn);
            double fy = F(Y / Yn);
            double fz = F(Z / Zn);

            double L = 116 * fy - 16;
            double a = 500 * (fx - fy);
            double b = 200 * (fy - fz);
            return (L, a, b);
        }
    }

    class ImgFile
    {
        public FileSystemEntity Src = null!;
        public string DstPath = null!;
        public string OkFileDigest = null!;
        public int Index = 0;
    }


    /// <summary>
    /// ImageSharp(Image&lt;L8&gt;) → OpenCvSharp(Mat) 変換  
    /// 返す <see cref="Mat"/> は 8-bit 1ch（CV_8UC1）なので、
    /// 「BGR → GRAY 変換後の Mat」と同じフォーマットになります。
    /// </summary>
    /// <remarks>
    /// * ImageSharp の画素バッファは連続メモリで確保されているので、  
    ///   <c>CopyPixelDataTo</c> で一括取得するのが最も簡潔です。<br/>
    /// * 取得した画素を OpenCV の Mat にコピーする際、  
    ///   行アライメント（<c>mat.Step()</c>）に注意します。<br/>
    /// * L8 → Mat の処理そのものは色空間変換を伴いません。  
    ///   BGR 3ch 画像が必要な場合は戻り値に対して  
    ///   <c>Cv2.CvtColor(gray, bgr, ColorConversionCodes.GRAY2BGR)</c> を実行してください。
    /// </remarks>
    public static Mat ImageSharpL8ToMat(Image<L8> image)
    {
        if (image == null)
            throw new ArgumentNullException(nameof(image));

        int width = image.Width;
        int height = image.Height;

        // ImageSharp → byte[]
        byte[] buffer = new byte[width * height];
        image.CopyPixelDataTo(buffer);

        // OpenCV Mat (CV_8UC1) を確保
        Mat mat = new Mat(height, width, MatType.CV_8UC1);

        int stride = (int)mat.Step(); // 行バイト数（4byte アライン）

        if (mat.IsContinuous() && stride == width)
        {
            // OpenCV 側にもパディングが無ければ一括コピー
            Marshal.Copy(buffer, 0, mat.Data, buffer.Length);
        }
        else
        {
            // 行パディングを考慮しながらコピー
            for (int y = 0; y < height; y++)
            {
                IntPtr dstRowPtr = mat.Data + y * stride;
                Marshal.Copy(buffer, y * width, dstRowPtr, width);
            }
        }

        return mat;
    }




    /// <summary>
    /// ImageSharp の 8-bit グレースケール <see cref="Image{L8}"/> を  
    /// 24-bit RGB (8-bit ×3) の <see cref="Image{Rgb24}"/> に変換するユーティリティ。
    /// </summary>
    /// <remarks>
    /// * <c>CloneAs&lt;Rgb24&gt;()</c> は ImageSharp 1.0 以降に標準実装されている
    ///   高速なピクセルフォーマット変換です。  
    /// * 各画素の輝度値 L が R, G, B のすべてのチャネルにコピーされます。  
    /// * αチャネルは存在しないので透過情報は含まれません。
    /// </remarks>
    public static Image<Rgb24> ImageL8ToRgb24(Image<L8> grayImage)
    {
        if (grayImage is null)
            throw new ArgumentNullException(nameof(grayImage));

        // L8 → Rgb24 変換
        return grayImage.CloneAs<Rgb24>();
    }

    /// <summary>
    /// OpenCvSharp(Mat) → ImageSharp(L8) 変換
    /// Mat にはあらかじめ <c>Cv2.CvtColor(src, dst, ColorConversionCodes.BGRA2GRAY)</c>
    /// などで 8bit 1ch (CV_8UC1) のグレースケール画像が入っている前提。
    /// </summary>
    /// <remarks>
    /// * Mat が連続メモリ（<see cref="Mat.IsContinuous"/>）でない場合に備えて
    ///   行ごとにコピーするフォールバックを入れています。<br/>
    /// * ImageSharp 1.0 以降は <see cref="Image.LoadPixelData{TPixel}"/> が推奨。<br/>
    /// * OpenCvSharp の Mat は行境界で 4 byte アラインされることがあるので、
    ///   <c>mat.Step()</c> を使ってストライドを取得しています。
    /// </remarks>
    public static Image<L8> MatToImageSharpL8(Mat mat)
    {
        if (mat.Empty())
            throw new ArgumentException("mat は空です", nameof(mat));

        if (mat.Type() != MatType.CV_8UC1)
            throw new ArgumentException("mat は CV_8UC1 (8bit 1ch) である必要があります", nameof(mat));

        int width = mat.Width;
        int height = mat.Height;
        int stride = (int)mat.Step();       // 1行のバイト数（幅≦stride、4byte アラインの可能性）

        // ImageSharp 側に渡すバッファ。幅 × 高さ 分のみ確保すれば OK（パディング不要）
        byte[] buffer = new byte[width * height];

        if (mat.IsContinuous())
        {
            // 連続メモリなら一括コピー
            Marshal.Copy(mat.Data, buffer, 0, buffer.Length);
        }
        else
        {
            // 行ごとにコピー（OpenCV のパディングをスキップ）
            for (int y = 0; y < height; y++)
            {
                IntPtr srcRowPtr = mat.Data + y * stride;
                Marshal.Copy(srcRowPtr, buffer, y * width, width);
            }
        }

        // ImageSharp へ取り込む（PixelType = L8／1byte）
        return Image.LoadPixelData<L8>(buffer, width, height);
    }

    /// <summary>
    /// OpenCvSharp(Mat) -> ImageSharp(Rgb24) 変換 (修正ポイント)
    /// </summary>
    public static SixLabors.ImageSharp.Image<Rgb24> MatToImageSharp(Mat mat)
    {
        // data は幅 * 高さ * 3(RGB)バイト
        int w = mat.Width;
        int h = mat.Height;

        byte[] data = new byte[w * h * 3];
        Marshal.Copy(mat.Data, data, 0, data.Length);

        // v1.0以降: Image.LoadPixelData で生成
        // PixelFormat = Rgba24
        var image = SixLabors.ImageSharp.Image.LoadPixelData<Rgb24>(data, w, h);
        return image;
    }

    /// <summary>
    /// ImageSharp(Rgba32) -> OpenCvSharp(Mat) 変換 (修正ポイント)
    /// </summary>
    public static Mat ImageSharpToMat(SixLabors.ImageSharp.Image<Rgb24> src)
    {
        int w = src.Width;
        int h = src.Height;
        // RGBでbyte3チャンネル
        byte[] bytes = new byte[w * h * 3];

        // v1.0以降: 行ごとにGetPixelRowSpan() で読み取り
        int offset = 0;
        src.ProcessPixelRows(target =>
        {
            for (int y = 0; y < h; y++)
            {
                var rowSpan = target.GetRowSpan(y);

                for (int x = 0; x < w; x++)
                {
                    var px = rowSpan[x];
                    bytes[offset + 0] = px.R;
                    bytes[offset + 1] = px.G;
                    bytes[offset + 2] = px.B;
                    offset += 3;
                }
            }
        });

        // ↓ このコンストラクタは内部用で直接呼べない場合あり
        // var mat = new Mat(h, w, MatType.CV_8UC4, bytes);

        //// 代替: 一度空の Mat を作って SetArray
        //var mat = new Mat(h, w, MatType.CV_8UC4);
        //mat.SetArray(bytes);

        // GCHandleで固定してポインタ渡し
        var mat = new Mat(h, w, MatType.CV_8UC3);
        Marshal.Copy(bytes, 0, mat.Data, bytes.Length);
        return mat;
    }


    /// <summary>
    /// 入力画像（srcWidth×srcHeight）をアスペクト比を保ったまま拡大し、
    /// targetScreenWidth×targetScreenHeight に対して scale=Math.Min(a1, a2*3) の法則で dstWidth, dstHeight を計算します。
    /// 拡大しない（元サイズのまま）場合は scale=1.0 になり、dstWidth=srcWidth, dstHeight=srcHeight となります。
    /// 戻り値は、dstWidth または dstHeight が元と変化したかどうかを示します。
    /// </summary>
    /// <param name="srcWidth">入力画像の横幅（ピクセル）</param>
    /// <param name="srcHeight">入力画像の縦幅（ピクセル）</param>
    /// <param name="targetScreenWidth">ターゲット画面の横幅（ピクセル）</param>
    /// <param name="targetScreenHeight">ターゲット画面の縦幅（ピクセル）</param>
    /// <param name="dstWidth">出力画像の横幅（ピクセル）</param>
    /// <param name="dstHeight">出力画像の縦幅（ピクセル）</param>
    /// <returns>
    /// dstWidth または dstHeight が srcWidth/scrHeight から変化していれば true、そうでなければ false
    /// </returns>
    public static bool CalcResizeImg(int srcWidth, int srcHeight, int targetScreenWidth, int targetScreenHeight, out int dstWidth, out int dstHeight)
    {
        // 1) 幅・高さそれぞれの拡大率を計算（double 型）
        double ratioW = (double)targetScreenWidth / srcWidth;   // 横方向に target を超えるための倍率
        double ratioH = (double)targetScreenHeight / srcHeight; // 縦方向に target を超えるための倍率

        // 2.1) a1 = 最大倍率（横幅と縦幅の両方を target 以上にする最小倍率）
        double a1 = Math.Max(ratioW, ratioH);
        if (a1 < 1.0)
        {
            // 縮小はしないので、1.0 未満なら 1.0 に丸める
            a1 = 1.0;
        }

        // 2.2) a2 = 最小倍率（横幅または縦幅のいずれか一方を先に target 以上にする最小倍率）
        double a2 = Math.Min(ratioW, ratioH);
        if (a2 < 1.0)
        {
            // 同様に、拡大しない場合は 1.0
            a2 = 1.0;
        }

        // 2.3) 最終スケール = Math.Min(a1, a2 * 3)
        double scale = Math.Min(a1, a2 * 3.0);

        // 3) dstWidth, dstHeight を計算
        //    ※ここではキャストによる小数切り捨てとしています。必要であれば Math.Ceiling/Math.Round に変更してください。
        dstWidth = (int)(srcWidth * scale);
        dstHeight = (int)(srcHeight * scale);

        // 4) 元のサイズから変わっていれば true、変わっていなければ false
        bool isChanged = (dstWidth != srcWidth) || (dstHeight != srcHeight);
        return isChanged;
    }
}

public class SuperPerformPdfOptions
{
    public int MarginPercent = 7;
    public int MaxPagesForDebug = int.MaxValue;
    public bool SaveDebugPng = false;
    public bool SkipRealesrgan = false;
}

public class SuperPdfResult
{
    public PnOcrLibBookMetaData? PnOcrMetaData;
    public SuperPerformPdfOptions? Options;
}

public static class SuperPdfUtil
{
    static readonly RefInt TmpCounter = new();

    public const int internalHighResImgWidth = 4960;
    public const int internalHighResImgHeight = 7016;

    public static async Task<bool> PerformPdfAsync(string srcPdfPath, string dstPdfPath, SuperPerformPdfOptions? options = null, bool useOkFile = true, CancellationToken cancel = default)
    {
        if (srcPdfPath._IsSamei(dstPdfPath))
        {
            throw new CoresLibException("srcPdfPath == dstPdfPath");
        }

        options ??= new();

        var srcPdfMetaData = await Lfs.GetFileMetadataAsync(srcPdfPath, cancel: cancel);
        string digest = $"{srcPdfMetaData.LastWriteTime!.Value.Ticks} {srcPdfMetaData.Size} {options._ObjectToJson()}"._Digest();

        if (useOkFile)
        {
            if (await Lfs.IsOkFileExistsAsync(dstPdfPath, digest, cancel: cancel))
            {
                Con.WriteLine($"PerformPdfAsync: '{srcPdfPath}' -> '{dstPdfPath}': Already exists. Skip.");
                return false;
            }
        }

        Con.WriteLine($"[PerformPdfAsync]: Starting '{srcPdfPath}' -> '{dstPdfPath}' ...");

        var result = await PerformPdfMainAsync(srcPdfPath, dstPdfPath, options, cancel: cancel);

        result.Options = options;

        if (useOkFile)
        {
            await Lfs.WriteOkFileAsync(dstPdfPath, result, digest, cancel: cancel);
        }

        Con.WriteLine($"[PerformPdfAsync]: Completed: '{srcPdfPath}' -> '{dstPdfPath}'");

        return true;
    }

    static async Task<SuperPdfResult> PerformPdfMainAsync(string srcPdfPath, string dstPdfPath, SuperPerformPdfOptions? options = null, CancellationToken cancel = default)
    {
        options ??= new();

        await Lfs.DeleteFileIfExistsAsync(dstPdfPath, raiseException: true, cancel: cancel);

        // 一時ディレクトリ
        string tmpDirRoot = PP.Combine(Env.MyLocalTempDir, "Perform_Pdf");

        if (await Lfs.IsDirectoryExistsAsync(tmpDirRoot))
        {
            await Lfs.DeleteDirectoryAsync(tmpDirRoot, true, cancel: cancel);
        }

        if (await Lfs.IsDirectoryExistsAsync(tmpDirRoot))
        {
            throw new CoresLibException($"Deleting the directory '{tmpDirRoot}' failed.");
        }

        string pdf_extracted_dir = PP.Combine(tmpDirRoot, "1_pdf_extracted_dir");
        await Lfs.CreateDirectoryAsync(pdf_extracted_dir, cancel: cancel);

        string pdf_extracted_dir2 = PP.Combine(tmpDirRoot, "1_2_pdf_extracted_dir2");
        await Lfs.CreateDirectoryAsync(pdf_extracted_dir2, cancel: cancel);

        string pdf_ai_result_dir = PP.Combine(tmpDirRoot, "2_pdf_ai_result_dir");
        await Lfs.CreateDirectoryAsync(pdf_ai_result_dir, cancel: cancel);

        string pdf_adjusted_dir = PP.Combine(tmpDirRoot, "3_pdf_adjusted_dir");
        await Lfs.CreateDirectoryAsync(pdf_adjusted_dir, cancel: cancel);

        string pdf_tmp_dir = PP.Combine(tmpDirRoot, "99_pdf_tmp_dir");
        await Lfs.CreateDirectoryAsync(pdf_tmp_dir, cancel: cancel);

        // PDF から画像を抽出
        ImageMagickExtractImageOption extractOptions = new ImageMagickExtractImageOption
        {
            Format = ImageMagickExtractImageFormat.Bmp,
            NumPages = options.MaxPagesForDebug,
        };
        await SuperBookExternalTools.ImageMagick.ExtractImagesFromPdfAsync(srcPdfPath, pdf_extracted_dir, extractOptions, cancel: cancel);

        // 抽出された画像の上下左右 0.5% をトリミングする (スキャンで黒枠などが映っている場合があるため)
        var bmpFiles = (await Lfs.EnumDirectoryAsync(pdf_extracted_dir, cancel: cancel)).Where(x => x.IsFile && x.Name._IsExtensionMatch(".bmp")).OrderBy(x => x.Name, StrCmpi);

        foreach (var bmpFile in bmpFiles)
        {
            using var srcImage = await SixLabors.ImageSharp.Image.LoadAsync<Rgb24>(bmpFile.FullPath, cancellationToken: cancel);

            if (srcImage.Width >= 10 && srcImage.Height >= 10)
            {
                int marginWidth = (int)((double)srcImage.Width * 0.005);
                int marginHeight = (int)((double)srcImage.Height * 0.005);

                var rect = new Rectangle(marginWidth, marginHeight, srcImage.Width - marginWidth * 2, srcImage.Height - marginHeight * 2);

                srcImage.Mutate(ctx =>
                {
                    ctx.Crop(rect);
                });

                await srcImage.SaveAsBmpAsync(PP.Combine(pdf_extracted_dir2, PP.GetFileName(bmpFile.FullPath)), cancellationToken: cancel);
            }
        }

        // realesrgan で鮮明化
        await using (var realesrgan = new AiUtilRealEsrganEngine(SuperBookExternalTools.Settings))
        {
            var aiOpt = new AiUtilRealEsrganPerformOption
            {
                OutScale = 2.0,
                //Model = "RealESRGAN_x4plus_anime_6B",
                Skip = options.SkipRealesrgan,
            };

            string ext = ".bmp";
            if (extractOptions.Format == ImageMagickExtractImageFormat.Png)
            {
                ext = ".png";
            }

            await realesrgan.PerformAsync(pdf_extracted_dir2, ext, pdf_ai_result_dir, aiOpt, cancel: cancel);
        }

        // 色調整・傾き修正
        var result = await PerformPagesYohakuAsync(pdf_ai_result_dir, pdf_adjusted_dir, pdf_tmp_dir, options.MarginPercent, Env.NumCpus, maxPagesForDebug: options.MaxPagesForDebug, saveDebugPng: options.SaveDebugPng);

        // PDF を生成
        await Lfs.DeleteFileIfExistsAsync(dstPdfPath, raiseException: true, cancel: cancel);
        await Lfs.EnsureCreateDirectoryForFileAsync(dstPdfPath, cancel: cancel);

        var pdfOpt = new ImageMagickBuildPdfOption
        {
        };

        int? pageShiftPhysicalPageNumberStart = null;
        int? pageShiftLogicalPageNumberStart = null;

        if (result != null)
        {
            var pages = result.Pages.OrderBy(x => x.PhysicalFileNumber).ToList();

            foreach (var page in pages)
            {
                if (page.LogicalPageNumber >= 1)
                {
                    if ((page.PhysicalFileNumber + 1) != page.LogicalPageNumber)
                    {
                        pageShiftPhysicalPageNumberStart = page.PhysicalFileNumber + 1;
                        pageShiftLogicalPageNumberStart = page.LogicalPageNumber;
                        break;
                    }
                }
            }
        }

        Con.WriteLine($"Building '{dstPdfPath}'...");
        await SuperBookExternalTools.ImageMagick.BuildPdfFromImagesAsync(pdf_adjusted_dir, dstPdfPath, pdfOpt,
            verticalWriting: result?.IsVerticalWriting ?? false,
            physicalPageStart: pageShiftPhysicalPageNumberStart, logicalPageStart: pageShiftLogicalPageNumberStart,
            cancel: cancel);

        Con.WriteLine($"Build '{dstPdfPath}' OK.");

        return new SuperPdfResult
        {
            PnOcrMetaData = result,
        };
    }

    /// <summary>
    /// 余白除去・色補正などを行うメイン関数。
    /// </summary>
    public static async Task<PnOcrLibBookMetaData?> PerformPagesYohakuAsync(
        string srcDir,
        string dstDir,
        string tmpDir,
        int marginPercent,
        int maxCpu,
        bool noOcr = false,
        bool noDeskew = false,
        int maxPagesForDebug = int.MaxValue,
        bool saveDebugPng = false
        )
    {
        if (maxPagesForDebug <= 0) maxPagesForDebug = int.MaxValue;

        // ---------------------------------------------------------
        // (0) tmpDir を掃除
        // ---------------------------------------------------------
        if (Directory.Exists(tmpDir))
        {
            foreach (var f in Directory.GetFiles(tmpDir))
            {
                File.Delete(f);
            }
        }
        else
        {
            Directory.CreateDirectory(tmpDir);
        }

        // ---------------------------------------------------------
        // (1) 入力PNGファイル一覧を取得し、ソートしてページリストを作る
        // ---------------------------------------------------------
        if (!Directory.Exists(srcDir))
        {
            throw new DirectoryNotFoundException($"srcDir not found: {srcDir}");
        }
        if (!Directory.Exists(dstDir))
        {
            Directory.CreateDirectory(dstDir);
        }

        var allSrcImgFiles = Directory.GetFiles(srcDir, "*", SearchOption.TopDirectoryOnly)
            .Where(x => x._IsExtensionMatch(".bmp .png"))
            .OrderBy(f => f) // ファイル名順にソート
            .Take(maxPagesForDebug) // debug
            .ToList();

        int totalPages = allSrcImgFiles.Count;
        if (totalPages == 0)
        {
            Console.WriteLine("No PNG files found in srcDir.");
            return null;
        }

        var pageInfos = allSrcImgFiles
            .Select((filePath, idx) => new PageInfo
            {
                FilePath = filePath,
                PageNumber = idx + 1,
                IsOdd = ((idx + 1) % 2 == 1)
            })
            .ToList();

        var oddGroup = pageInfos.Where(p => p.IsOdd).ToList();
        var evenGroup = pageInfos.Where(p => !p.IsOdd).ToList();

        // ---------------------------------------------------------
        // (2) 全ページを走査して deskew(傾き補正) → カラー統計抽出 → 一時保存
        // ---------------------------------------------------------
        var semaphore = new SemaphoreSlim(maxCpu, maxCpu);
        //var semaphore = new SemaphoreSlim(1, 1);
        var colorStatsList_Even = new ConcurrentBag<ColorStats>();
        var colorStatsList_Odd = new ConcurrentBag<ColorStats>();

        await Task.WhenAll(pageInfos.Select(async page =>
        {
            Con.WriteLine($"Loading " + page.FilePath);
            await semaphore.WaitAsync().ConfigureAwait(false);
            try
            {
                // 1ページの画像読み込み
                using var srcImage = await SixLabors.ImageSharp.Image.LoadAsync<Rgba32>(page.FilePath);

                // ---------------------------------------------------------
                //  (1.5) 画像をターゲットサイズ (4960x7016) にフィットさせる
                //         ・縦横同倍率で拡大 / 縮小
                //         ・余った領域は紙色で塗りつぶし
                // ---------------------------------------------------------
                // ★ ここに挿入 ★

                // 元画像をフィットさせるスケール (縦横同倍率)
                double scale = Math.Min(
                    (double)internalHighResImgWidth / srcImage.Width,
                    (double)internalHighResImgHeight / srcImage.Height);

                int fittedW = (int)Math.Round(srcImage.Width * scale);
                int fittedH = (int)Math.Round(srcImage.Height * scale);

                /*
                Rgba32 paperColor = EstimatePaperColor(srcImage);

                // 元画像をスケール変換
                srcImage.Mutate(ctx =>
                {
                    //ctx.Resize(new ResizeOptions
                    //{
                    //    Size = new SixLabors.ImageSharp.Size(fittedW, fittedH),
                    //    Mode = ResizeMode.BoxPad,      // 縦横比維持でフィット
                    //    Sampler = KnownResamplers.Lanczos3,
                    //    PremultiplyAlpha = true
                    //});
                    ctx.Resize(fittedW, fittedH, KnownResamplers.Lanczos3);
                });

                // スケール後の画像を、ターゲットサイズのキャンバスへ貼り付ける
                using var newImage = new Image<Rgba32>(TargetWidth, TargetHeight, paperColor);
                int offsetX = (TargetWidth - fittedW) / 2;
                int offsetY = (TargetHeight - fittedH) / 2;

                newImage.Mutate(ctx =>
                {
                    ctx.DrawImage(srcImage, new SixLabors.ImageSharp.Point(offsetX, offsetY), 1f);
                });*/

                using var newImage = ResizeAndMakePaddingWithNaturalPaperColor(srcImage,
                                              internalHighResImgWidth, internalHighResImgHeight);


                // ---------------------------------------------------------
                // ここまで追加
                // ---------------------------------------------------------


                // 傾き補正(OpenCvSharp使用)
                using var deskewedImage = noDeskew == false ? (await DeskewImageWithOpenCvAsync(newImage)) : newImage.Clone();

                // deskew後ファイルを tmpDir に保存
                string tempFilePath = PP.Combine(tmpDir, $"deskew_{page.PageNumber:D4}.png");
                await deskewedImage.SaveAsync(tempFilePath, new PngEncoder());

                // カラー統計情報抽出
                var stats = CalculateColorStats(deskewedImage);
                stats.PageNumber = page.PageNumber;

                if ((page.PageNumber % 2) == 0)
                {
                    colorStatsList_Even.Add(stats);
                }
                else
                {
                    colorStatsList_Odd.Add(stats);
                }
            }
            catch (Exception ex)
            {
                ex._Error();
                throw;
            }
            finally
            {
                semaphore.Release();
            }
        }));

        // ---------------------------------------------------------
        // (3) カラー統計情報から外れ値除外し、
        //     書籍全体に対しての共通カラー補正パラメータを決定
        // ---------------------------------------------------------
        var colorStatsAll_Even = colorStatsList_Even.ToList();
        var colorStatsAll_Odd = colorStatsList_Odd.ToList();

        var filteredStats_Even = ExcludeOutliers(colorStatsAll_Even);
        var filteredStats_Odd = ExcludeOutliers(colorStatsAll_Odd);

        var globalColorParam_Even = DecideGlobalColorAdjustment(filteredStats_Even);
        var globalColorParam_Odd = DecideGlobalColorAdjustment(filteredStats_Odd);

        // ---------------------------------------------------------
        // (4) グローバルカラー補正 + 文字領域検出(BBox)
        // ---------------------------------------------------------
        var oddBoundingBoxes = new ConcurrentBag<PageBoundingBox>();
        var evenBoundingBoxes = new ConcurrentBag<PageBoundingBox>();

        await Task.WhenAll(pageInfos.Select(async page =>
        {
            await semaphore.WaitAsync().ConfigureAwait(false);
            try
            {
                Con.WriteLine($"Processing " + page.FilePath);

                // deskew後ファイル読み込み
                string deskewFilePath = PP.Combine(tmpDir, $"deskew_{page.PageNumber:D4}.png");
                //if (!File.Exists(deskewFilePath))
                //{
                //    return;
                //}
                using var deskewedImage = await SixLabors.ImageSharp.Image.LoadAsync<Rgba32>(deskewFilePath);

                // グローバルカラー補正を適用
                if ((page.PageNumber % 2) == 0)
                {
                    ApplyGlobalColorAdjustment(deskewedImage, globalColorParam_Even);
                }
                else
                {
                    ApplyGlobalColorAdjustment(deskewedImage, globalColorParam_Odd);
                }

                // 文字領域のBBox検出
                var bbox = DetectTextBoundingBox(deskewedImage);
                if (page.IsOdd)
                {
                    oddBoundingBoxes.Add(new PageBoundingBox
                    {
                        PageNumber = page.PageNumber,
                        SrcPath = page.FilePath,
                        BoundingBox = bbox
                    });
                }
                else
                {
                    evenBoundingBoxes.Add(new PageBoundingBox
                    {
                        PageNumber = page.PageNumber,
                        SrcPath = page.FilePath,
                        BoundingBox = bbox
                    });
                }

                // 補正済み画像を tmpDir に保存 (最終拡大前段階)
                string colorAdjustedFilePath = PP.Combine(tmpDir, $"coloradj_{page.PageNumber:D4}.png");
                page.FilePathColorAdj = colorAdjustedFilePath;
                await deskewedImage.SaveAsync(colorAdjustedFilePath, new PngEncoder());
            }
            finally
            {
                semaphore.Release();
            }
        }));

        // ページ番号 OCR
        PnOcrLibBookMetaData? pnOcrResult = null;

        if (noOcr == false)
        {
            pnOcrResult = await PnOcrLib.OcrProcessForBookAsync(pageInfos.Select(x => x.FilePathColorAdj), saveDebugPng);
        }

        // ---------------------------------------------------------
        // (5) 奇数ページグループ、偶数ページグループで BBox を集計し、
        //     外れ値除外のうえ「一律削除すべき余白領域」を決定
        // ---------------------------------------------------------
        var oddCropRegion = DecideGroupCropRegion(oddBoundingBoxes.OrderBy(x => x.PageNumber).ToList());
        var evenCropRegion = DecideGroupCropRegion(evenBoundingBoxes.OrderBy(x => x.PageNumber).ToList());

        // Y 座標については余白の小さいほうを選択し左右両ページを統一 2025/07/06 追加
        //List<PageBoundingBox> allBoundingBoxes = new List<PageBoundingBox>();
        //allBoundingBoxes.AddRange(oddBoundingBoxes);
        //allBoundingBoxes.AddRange(evenBoundingBoxes);
        //var allCropRegion = DecideGroupCropRegion(allBoundingBoxes);
        int totalCropTop = Math.Min(oddCropRegion.Top, evenCropRegion.Top);
        int totalCropBottom = Math.Max(oddCropRegion.Bottom, evenCropRegion.Bottom);
        oddCropRegion = new Rectangle(oddCropRegion.X, totalCropTop, oddCropRegion.Width, totalCropBottom - totalCropTop);
        evenCropRegion = new Rectangle(evenCropRegion.X, totalCropTop, evenCropRegion.Width, totalCropBottom - totalCropTop);

        // 奇数・偶数のクロップ領域サイズを揃える（拡大する） (奇数 / 偶数グループ間で調整して、文字サイズを同一にするため)
        // 最大幅・高さを求める
        int maxWidth = Math.Max(oddCropRegion.Width, evenCropRegion.Width);
        int maxHeight = Math.Max(oddCropRegion.Height, evenCropRegion.Height);

        maxWidth += maxWidth * marginPercent / 100;
        maxHeight += maxHeight * marginPercent / 100;

        // oddCropRegion の調整
        if (oddCropRegion.Width < maxWidth || oddCropRegion.Height < maxHeight)
        {
            int dw = maxWidth - oddCropRegion.Width;
            int dh = maxHeight - oddCropRegion.Height;
            int newLeft = oddCropRegion.Left - dw / 2;
            int newTop = oddCropRegion.Top - dh / 2;

            // https://chatgpt.com/c/6847e6d5-2520-8008-826f-67cd9ac936a3
            maxWidth = Math.Min(maxWidth, internalHighResImgWidth);
            newLeft = Math.Clamp(newLeft, 0, internalHighResImgWidth - maxWidth);

            maxHeight = Math.Min(maxHeight, internalHighResImgHeight);
            newTop = Math.Clamp(newTop, 0, internalHighResImgHeight - maxHeight);

            oddCropRegion = new Rectangle(
                newLeft,
                newTop,
                maxWidth,
                maxHeight
            );
        }

        // evenCropRegion の調整
        if (evenCropRegion.Width < maxWidth || evenCropRegion.Height < maxHeight)
        {
            int dw = maxWidth - evenCropRegion.Width;
            int dh = maxHeight - evenCropRegion.Height;
            int newLeft = evenCropRegion.Left - dw / 2;
            int newTop = evenCropRegion.Top - dh / 2;

            // https://chatgpt.com/c/6847e6d5-2520-8008-826f-67cd9ac936a3
            maxWidth = Math.Min(maxWidth, internalHighResImgWidth);
            newLeft = Math.Clamp(newLeft, 0, internalHighResImgWidth - maxWidth);

            maxHeight = Math.Min(maxHeight, internalHighResImgHeight);
            newTop = Math.Clamp(newTop, 0, internalHighResImgHeight - maxHeight);

            evenCropRegion = new Rectangle(
                newLeft,
                newTop,
                maxWidth,
                maxHeight
            );
        }

        // 出力画像サイズの計算
        //const int finalTargetWidth_Standard = 2480;
        const int finalTargetHeight_Standard = 3508;

        int tmpWidth = Math.Max(oddCropRegion.Width, evenCropRegion.Width);
        int tmpHeight = Math.Max(oddCropRegion.Height, evenCropRegion.Height);

        int finalHeight = finalTargetHeight_Standard;
        int finalWidth = tmpWidth * finalTargetHeight_Standard / tmpHeight;

        // ---------------------------------------------------------
        // (6) 最終出力: crop + マージン + リサイズ(元のサイズに戻す) + 書き出し
        // ---------------------------------------------------------
        await Task.WhenAll(pageInfos.Select(async page =>
        {
            await semaphore.WaitAsync().ConfigureAwait(false);
            try
            {
                Con.WriteLine($"Finalizing " + page.FilePath);

                PnOcrLibPageMetaData? pnOcrPageResult = null;
                if (pnOcrResult != null)
                {
                    pnOcrPageResult = pnOcrResult.Pages.Where(x => x.PhysicalFileNumber == (page.PageNumber - 1)).FirstOrDefault();
                }


                // カラー調整済み画像を読み込む
                string colorAdjustedFilePath = PP.Combine(tmpDir, $"coloradj_{page.PageNumber:D4}.png");
                //if (!File.Exists(colorAdjustedFilePath))
                //{
                //    return;
                //}
                using var colorAdjustedImg = await SixLabors.ImageSharp.Image.LoadAsync<Rgba32>(colorAdjustedFilePath);

                // グループ別クロップ領域
                var cropRegion = page.IsOdd ? oddCropRegion : evenCropRegion;

                // 正規化
                var actualCrop = AddMarginAndClip(cropRegion, 0,
                                                  colorAdjustedImg.Width, colorAdjustedImg.Height);

                //// Crop
                //colorAdjustedImg.Mutate(ctx =>
                //{
                //    ctx.Crop(actualCrop);
                //});


                // 紙の色を補間しつつ拡大縮小
                using var finalImg = ResizeAndMakePaddingWithNaturalPaperColor2(colorAdjustedImg,
                                              finalWidth,
                                              finalHeight,
                                              -actualCrop.Left + (pnOcrPageResult?.Shift_X ?? 0),
                                              -actualCrop.Top + (pnOcrPageResult?.Shift_Y ?? 0),
                                              (double)finalWidth / (double)actualCrop.Width
                                              );

                //// 一応紙の色を補間しつつ拡大縮小 ただ、この時点ではピッタリ合っているはずなので実質的に意味がないかも
                //using var finalImg = ResizeAndMakePaddingWithNaturalPaperColor(colorAdjustedImg,
                //                              finalWidth, finalHeight,
                //                              shiftX: pnOcrPageResult?.Shift_X ?? 0, // ページ番号 OCR 結果に基づくシフト
                //                              shiftY: pnOcrPageResult?.Shift_Y ?? 0
                //                              );

                // 出力ファイル名
                string dstFileName = PP.GetFileName(page.FilePath + ".png");
                string dstPath = PP.Combine(dstDir, dstFileName);
                await finalImg.SaveAsync(dstPath, new PngEncoder());
            }
            finally
            {
                semaphore.Release();
            }
        }));

        Console.WriteLine("All pages processed successfully.");

        return pnOcrResult;
    }

    // -----------------------------------------------------------------------------
    // 画像全体から「紙色」を推定する。
    // 1. 2ピクセルごとに間引き走査して輝度ヒストグラム (0–255) を作成
    // 2. ヒストグラムの上位 5 % の輝度帯を「十分に明るい画素」とみなす
    // 3. その中でも彩度が低い (≒無彩色に近い) 画素だけを抽出
    // 4. 抽出画素の平均 RGB を紙色として返す
    //    ─ 抽出画素が 0 なら純白 (255,255,255) を返す
    // -----------------------------------------------------------------------------
    static Rgba32 EstimatePaperColor(Image<Rgba32> img)
    {
        int w = img.Width;
        int h = img.Height;

        // (1) 256bin ヒストグラムを作成 (2px 間隔でサンプリング)
        Memory<int> lumHist = new int[256];
        long samplePix = 0;
        var lumHistSpan = lumHist.Span;

        img.ProcessPixelRows(accessor =>
        {
            var lumHistSpan = lumHist.Span;
            for (int y = 0; y < h; y += 2)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x += 2)
                {
                    var p = row[x];
                    int lum = (p.R * 299 + p.G * 587 + p.B * 114) / 1000; // ITU-R BT.601
                    lumHistSpan[lum]++;
                    samplePix++;
                }
            }
        });

        // (2) 上位 5 % (＝最も明るい側) を求める
        long target = (long)(samplePix * 0.05);
        long acc = 0;
        int lumThreshold = 255;
        for (int i = 255; i >= 0; i--)
        {
            acc += lumHistSpan[i];
            if (acc >= target)
            {
                lumThreshold = i;
                break;
            }
        }

        // (3) 低彩度画素 (sat < 40) だけを集計
        long sumR = 0, sumG = 0, sumB = 0, cnt = 0;

        img.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < h; y += 2)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x += 2)
                {
                    var p = row[x];
                    int lum = (p.R * 299 + p.G * 587 + p.B * 114) / 1000;
                    if (lum < lumThreshold) continue;

                    int max = Math.Max(p.R, Math.Max(p.G, p.B));
                    int min = Math.Min(p.R, Math.Min(p.G, p.B));
                    int sat = max == 0 ? 0 : (max - min) * 255 / max; // 0–255

                    if (sat < 40)   // 彩度が高い (色付き) 画素は除外
                    {
                        sumR += p.R;
                        sumG += p.G;
                        sumB += p.B;
                        cnt++;
                    }
                }
            }
        });

        if (cnt == 0) return new Rgba32(255, 255, 255);   // フォールバック

        byte r = (byte)(sumR / cnt);
        byte g = (byte)(sumG / cnt);
        byte b = (byte)(sumB / cnt);
        return new Rgba32(r, g, b);
    }


    #region (A) Deskew (OpenCvSharp) - 修正ポイントあり
    /// <summary>
    /// 画像の傾きをOpenCvSharpで検出し、補正したImageSharp Imageを返す。
    /// </summary>
    private static async Task<SixLabors.ImageSharp.Image<Rgba32>> DeskewImageWithOpenCvAsync(SixLabors.ImageSharp.Image<Rgba32> src)
    {
        // 傾き角度を検出
        string otsuImgTmpPngPath = await Lfs.GenerateUniqueTempFilePathAsync("deskew", ".png");

        // 検知用画像は大津 2 値化した結果とする
        using var otsuImg = PnOcrLib.PerformOtsuForPaperPage(src);

        await otsuImg.SaveAsPngAsync(otsuImgTmpPngPath);

        try
        {
            // 2 値化されたイメージを用いて傾きを検出
            double angle = await SuperBookExternalTools.ImageMagick.GetDeskewRotateDegreeAsync(otsuImgTmpPngPath, -1);

            if (angle._IsNearlyZero())
            {
                // 回転なし
                return src;
            }

            angle = -angle;

            // ImageSharp -> OpenCvSharp(Mat) に変換 (修正)
            using var mat = ImageSharpToMat(src);

            // 回転行列を使って回転補正
            Point2f center = new Point2f(mat.Width / 2.0f, mat.Height / 2.0f);
            using Mat rotMat = Cv2.GetRotationMatrix2D(center, angle, 1.0);

            using var rotated = new Mat();
            // 修正: BorderTypes.Constant + Scalar.White で背景を白に
            Cv2.WarpAffine(
                mat,
                rotated,
                rotMat,
                new OpenCvSharp.Size(mat.Width, mat.Height),
                InterpolationFlags.Linear,
                BorderTypes.Constant,
                new Scalar(255, 255, 255, 255)  // 白
            );

            // Mat -> ImageSharp 変換 (修正)
            var result = MatToImageSharp(rotated);
            // mat, rotMat は using で破棄される

            return result;
        }
        finally
        {
            try
            {
                await Lfs.DeleteFileIfExistsAsync(otsuImgTmpPngPath);
            }
            catch { }
        }
    }

    /// <summary>
    /// ImageSharp(Rgba32) -> OpenCvSharp(Mat) 変換 (修正ポイント)
    /// </summary>
    public static Mat ImageSharpToMat(SixLabors.ImageSharp.Image<Rgba32> src)
    {
        int w = src.Width;
        int h = src.Height;
        // RGBAでbyte4チャンネル
        byte[] bytes = new byte[w * h * 4];

        // v1.0以降: 行ごとにGetPixelRowSpan() で読み取り
        int offset = 0;
        src.ProcessPixelRows(target =>
        {
            for (int y = 0; y < h; y++)
            {
                var rowSpan = target.GetRowSpan(y);

                for (int x = 0; x < w; x++)
                {
                    var px = rowSpan[x];
                    bytes[offset + 0] = px.R;
                    bytes[offset + 1] = px.G;
                    bytes[offset + 2] = px.B;
                    bytes[offset + 3] = px.A;
                    offset += 4;
                }
            }
        });

        // ↓ このコンストラクタは内部用で直接呼べない場合あり
        // var mat = new Mat(h, w, MatType.CV_8UC4, bytes);

        //// 代替: 一度空の Mat を作って SetArray
        //var mat = new Mat(h, w, MatType.CV_8UC4);
        //mat.SetArray(bytes);

        // GCHandleで固定してポインタ渡し
        var mat = new Mat(h, w, MatType.CV_8UC4);
        Marshal.Copy(bytes, 0, mat.Data, bytes.Length);
        return mat;
    }

    /// <summary>
    /// OpenCvSharp(Mat) -> ImageSharp(Rgba32) 変換 (修正ポイント)
    /// </summary>
    private static SixLabors.ImageSharp.Image<Rgba32> MatToImageSharp(Mat mat)
    {
        // data は幅 * 高さ * 4(RGBA)バイト
        int w = mat.Width;
        int h = mat.Height;

        byte[] data = new byte[w * h * 4];
        Marshal.Copy(mat.Data, data, 0, data.Length);

        // v1.0以降: Image.LoadPixelData で生成
        // PixelFormat = Rgba32
        var image = SixLabors.ImageSharp.Image.LoadPixelData<Rgba32>(data, w, h);
        return image;
    }

    #endregion

    #region (B) カラー統計と補正

    /// <summary>
    /// 1ページのカラー統計値を計算 (簡易実装)。
    /// </summary>
    private static ColorStats CalculateColorStats(Image<Rgba32> image)
    {
        // ── 1. まず輝度ヒストグラムを粗く取る ──────────────────────────────
        const int SAMPLE_STEP = 4;           // 全画素を読むと遅いので 1/16 に間引き
        Memory<long> hist = new long[256];
        var histSpan = hist.Span;
        long total = 0;

        int w = image.Width;
        int h = image.Height;
        image.ProcessPixelRows(accessor =>
        {
            var histSpan = hist.Span;
            for (int y = 0; y < h; y += SAMPLE_STEP)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x += SAMPLE_STEP)
                {
                    var p = row[x];
                    int y8 = (int)(0.299 * p.R + 0.587 * p.G + 0.114 * p.B + 0.5);
                    histSpan[y8]++;
                    total++;
                }
            }
        });

        // ── 2. 5 % 点 (インク側) と 95 % 点 (紙側) を抽出 ──────────────────
        long lowTarget = (long)(total * 0.05);
        long highTarget = (long)(total * 0.95);

        int lowLum = 0, highLum = 255;
        long acc = 0;
        for (int i = 0; i < 256; i++)
        {
            acc += histSpan[i];
            if (acc >= lowTarget && lowLum == 0) lowLum = i;
            if (acc >= highTarget) { highLum = i; break; }
        }

        // ── 3. それぞれの RGB 平均を計算 ──────────────────────────────────
        long sumPaperR = 0, sumPaperG = 0, sumPaperB = 0, cntPaper = 0;
        long sumInkR = 0, sumInkG = 0, sumInkB = 0, cntInk = 0;

        image.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < h; y += SAMPLE_STEP)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x += SAMPLE_STEP)
                {
                    var p = row[x];
                    int y8 = (int)(0.299 * p.R + 0.587 * p.G + 0.114 * p.B + 0.5);

                    if (y8 >= highLum)        // 紙
                    {
                        sumPaperR += p.R; sumPaperG += p.G; sumPaperB += p.B;
                        cntPaper++;
                    }
                    else if (y8 <= lowLum)    // インク
                    {
                        sumInkR += p.R; sumInkG += p.G; sumInkB += p.B;
                        cntInk++;
                    }
                }
            }
        });

        // ダミー防止
        if (cntPaper == 0) cntPaper = 1;
        if (cntInk == 0) cntInk = 1;

        // ── 4. 統計を返す ────────────────────────────────────────────────
        return new ColorStats
        {
            // 既存プロパティは「紙（背景）」平均を詰めて互換維持
            MeanR = sumPaperR / (double)cntPaper,
            MeanG = sumPaperG / (double)cntPaper,
            MeanB = sumPaperB / (double)cntPaper,

            // ★ 新規プロパティ（下記クラス変更参照）
            PaperR = sumPaperR / (double)cntPaper,
            PaperG = sumPaperG / (double)cntPaper,
            PaperB = sumPaperB / (double)cntPaper,
            InkR = sumInkR / (double)cntInk,
            InkG = sumInkG / (double)cntInk,
            InkB = sumInkB / (double)cntInk
        };
    }
    private static List<ColorStats> ExcludeOutliers(List<ColorStats> list)
    {
        if (list.Count < 3) return list;

        // 例: 平均Rの上下20%を除外
        var sorted = list.OrderBy(s => s.MeanR).ToList();
        int skip = (int)(sorted.Count * 0.20);
        int take = sorted.Count - skip * 2;
        if (take < 1) take = 1;
        var truncated = sorted.Skip(skip).Take(take).ToList();
        return truncated;
    }

    private static GlobalColorParam DecideGlobalColorAdjustment(List<ColorStats> statsList)
    {
        // フォールバック
        if (statsList.Count == 0)
            return new GlobalColorParam
            {
                ScaleR = 1,
                ScaleG = 1,
                ScaleB = 1,
                OffsetR = 0,
                OffsetG = 0,
                OffsetB = 0,
                GhostSuppressLumThreshold = 200,
                WhiteClipRange = 30
            };

        /*───────────────────────────────────────────────────────
         * 1) ページ外れ値除去  ― 従来どおり（中央値＋MAD）
         *───────────────────────────────────────────────────────*/
        IEnumerable<double> paperY = statsList.Select(s =>
            0.299 * s.PaperR + 0.587 * s.PaperG + 0.114 * s.PaperB);
        double medY = Percentile(paperY.ToList(), 50);
        double mad = Percentile(paperY.Select(v => Math.Abs(v - medY)).ToList(), 50);
        double thr = mad * 1.5;

        var mainPages = statsList.Where(s =>
            Math.Abs((0.299 * s.PaperR + 0.587 * s.PaperG + 0.114 * s.PaperB) - medY) <= thr)
            .ToList();
        if (mainPages.Count == 0) mainPages = statsList;

        /*───────────────────────────────────────────────────────
         * 2) 背景(paper) / 前景(ink) チャンネル別中央値
         *───────────────────────────────────────────────────────*/
        double bgR = Percentile(mainPages.Select(s => s.PaperR).ToList(), 50);
        double bgG = Percentile(mainPages.Select(s => s.PaperG).ToList(), 50);
        double bgB = Percentile(mainPages.Select(s => s.PaperB).ToList(), 50);

        double inkR = Percentile(mainPages.Select(s => s.InkR).ToList(), 50);
        double inkG = Percentile(mainPages.Select(s => s.InkG).ToList(), 50);
        double inkB = Percentile(mainPages.Select(s => s.InkB).ToList(), 50);

        /*───────────────────────────────────────────────────────
         * 3) 線形スケール     ink→0,  paper→255
         *    ― 拡大率が大きくなり過ぎると裏移りが強調されるので
         *      0.8〜2.0 の範囲に抑制
         *───────────────────────────────────────────────────────*/
        static (double s, double o) Lin(double bg, double ink)
        {
            double diff = bg - ink;
            if (diff < 1) return (1, 0);           // 万一同値の場合
            double s = Math.Clamp(255.0 / diff, 0.8, 4.0);
            double o = -ink * s;
            return (s, o);
        }

        var (sR, oR) = Lin(bgR, inkR);
        var (sG, oG) = Lin(bgG, inkG);
        var (sB, oB) = Lin(bgB, inkB);

        /*───────────────────────────────────────────────────────
         * 4) 裏写り抑制しきい値
         *    線形補正「後」の輝度を想定して計算
         *───────────────────────────────────────────────────────*/
        byte ScClamp(double v) => (byte)Math.Clamp(v, 0, 255);
        double bgLumScaled =
            0.299 * ScClamp(bgR * sR + oR) +
            0.587 * ScClamp(bgG * sG + oG) +
            0.114 * ScClamp(bgB * sB + oB);

        double inkLumScaled =
            0.299 * ScClamp(inkR * sR + oR) +
            0.587 * ScClamp(inkG * sG + oG) +
            0.114 * ScClamp(inkB * sB + oB);

        // 「紙と文字のちょうど中間」を裏写り抑制しきい値に
        byte ghostThr = (byte)Math.Clamp(
            (inkLumScaled + bgLumScaled) * 0.5, 0, 255);

        /* 4) 背景代表色と抑制しきい値などを追加      */

        var pOut = new GlobalColorParam
        {
            ScaleR = sR,
            OffsetR = oR,
            ScaleG = sG,
            OffsetG = oG,
            ScaleB = sB,
            OffsetB = oB,

            GhostSuppressLumThreshold = ghostThr,
            WhiteClipRange = 30,

            PaperR = (byte)Math.Round(bgR),
            PaperG = (byte)Math.Round(bgG),
            PaperB = (byte)Math.Round(bgB),

            // [11][12] で調整した彩度・色差閾値
            SatThreshold = 55,
            ColorDistThreshold = 35,

            // －－ これだけ変える －－
            BleedHueMin = 20f,   // 20° (黄寄り)
            BleedHueMax = 65f,   // 65° (橙寄り)
            BleedValueMin = 0.35f, // 明度最低35％まで許容
                                   // sat 閾値は使わず、常に全彩度に適用させる
        };
        return pOut;
    }

    // [共通 util] シンプルな百分位点
    private static double Percentile(List<double> list, double p)
    {
        if (list.Count == 0) return 0;
        list.Sort();
        double rank = (p / 100.0) * (list.Count - 1);
        int lo = (int)Math.Floor(rank);
        int hi = (int)Math.Ceiling(rank);
        if (lo == hi) return list[lo];
        return list[lo] + (list[hi] - list[lo]) * (rank - lo);
    }


    /// <summary>
    /// ImageSharp のみで、(1) 背景紙寄り画素を smooth‐step で白化し
    /// (2) オレンジ・ピンクの小ゴミ (#F4E7E9 相当) を赤桃色 Hue 範囲に絞って完全白化
    /// を行います。薄い青 (#DFF2FF/#E8F5FF) は残ります。
    /// </summary>
    private static void ApplyGlobalColorAdjustment(Image<Rgba32> image, GlobalColorParam p)
    {
        static byte Clamp8(double v) => (byte)(v < 0 ? 0 : (v > 255 ? 255 : v));

        int w = image.Width, h = image.Height;
        byte paperR = p.PaperR, paperG = p.PaperG, paperB = p.PaperB;
        int clipStart = p.GhostSuppressLumThreshold;
        int clipEnd = Math.Clamp(255 - p.WhiteClipRange, 0, 255);

        image.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < h; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x++)
                {
                    var src = row[x];
                    // 1) 線形補正
                    byte r = Clamp8(src.R * p.ScaleR + p.OffsetR);
                    byte g = Clamp8(src.G * p.ScaleG + p.OffsetG);
                    byte b = Clamp8(src.B * p.ScaleB + p.OffsetB);

                    // 2) 紙寄り画素の smooth‐step 白化
                    int lum = (r * 299 + g * 587 + b * 114) / 1000;
                    if (lum >= clipStart)
                    {
                        int maxc = Math.Max(r, Math.Max(g, b));
                        int minc = Math.Min(r, Math.Min(g, b));
                        int sat = maxc == 0 ? 0 : (maxc - minc) * 255 / maxc;
                        int dist = Math.Abs(r - paperR)
                                 + Math.Abs(g - paperG)
                                 + Math.Abs(b - paperB);

                        if (sat < p.SatThreshold && dist < p.ColorDistThreshold)
                        {
                            double t = Math.Clamp((double)(lum - clipStart) / (clipEnd - clipStart + 1e-6), 0.0, 1.0);
                            double wgt = t * t * (3.0 - 2.0 * t);
                            r = Clamp8(r + (255 - r) * wgt);
                            g = Clamp8(g + (255 - g) * wgt);
                            b = Clamp8(b + (255 - b) * wgt);
                        }
                    }

                    // 3) オレンジ／ピンク小ゴミを赤桃色 Hue 範囲に絞って完全白化
                    //    Hue 0–40° or 330–360°
                    RgbToHsv(r, g, b, out float hue, out _, out float val);
                    int max2 = Math.Max(r, Math.Max(g, b));
                    int min2 = Math.Min(r, Math.Min(g, b));
                    int sat2 = max2 == 0 ? 0 : (max2 - min2) * 255 / max2;
                    int lum2 = (r * 299 + g * 587 + b * 114) / 1000;

                    bool isPastelPink =
                        lum2 > 230 &&
                        sat2 < 30 &&
                        (hue <= 40f || hue >= 330f);

                    if (isPastelPink)
                    {
                        r = g = b = 255;
                    }

                    row[x] = new Rgba32(r, g, b, src.A);
                }
            }
        });
    }

    /// <summary>
    /// RGB → HSV 変換ヘルパー (h: 0–360°, s/v: 0–1)
    /// </summary>
    private static void RgbToHsv(byte r, byte g, byte b, out float h, out float s, out float v)
    {
        float rf = r / 255f, gf = g / 255f, bf = b / 255f;
        float max = MathF.Max(rf, MathF.Max(gf, bf));
        float min = MathF.Min(rf, MathF.Min(gf, bf));
        v = max;
        float d = max - min;
        s = max == 0 ? 0 : d / max;
        if (d == 0)
        {
            h = 0;
        }
        else if (max == rf)
        {
            h = 60f * (((gf - bf) / d) % 6f);
        }
        else if (max == gf)
        {
            h = 60f * (((bf - rf) / d) + 2f);
        }
        else
        {
            h = 60f * (((rf - gf) / d) + 4f);
        }
        if (h < 0) h += 360f;
    }




    #endregion

    #region (C) 余白検出(文字領域)

    // ---------------------------------------------------------
    // (C) 余白検出 (文字領域)
    // ---------------------------------------------------------
    /// <summary>
    /// 文字領域を自動検出し、描画可能なバウンディングボックスを返す。
    /// </summary>
    private static Rectangle DetectTextBoundingBox(SixLabors.ImageSharp.Image<Rgba32> image)
    {
        // 1. ImageSharp -> OpenCvSharp Mat (RGBA)
        using var mat = ImageSharpToMat(image);

        // 2. グレースケール化
        using var gray = new Mat();
        Cv2.CvtColor(mat, gray, ColorConversionCodes.BGRA2GRAY);

        // 3. 余白とみなす上下左右1%のエリアは影やノイズを無視
        int borderX = Math.Max(gray.Cols / 100, 1);
        int borderY = Math.Max(gray.Rows / 100, 1);
        // 上
        Cv2.Rectangle(gray, new OpenCvSharp.Rect(0, 0, gray.Cols, borderY), new Scalar(255), -1);
        // 下
        Cv2.Rectangle(gray, new OpenCvSharp.Rect(0, gray.Rows - borderY, gray.Cols, borderY), new Scalar(255), -1);
        // 左
        Cv2.Rectangle(gray, new OpenCvSharp.Rect(0, 0, borderX, gray.Rows), new Scalar(255), -1);
        // 右
        Cv2.Rectangle(gray, new OpenCvSharp.Rect(gray.Cols - borderX, 0, borderX, gray.Rows), new Scalar(255), -1);

        // 4. Otsuの二値化（背景：白=255→テキスト部が黒=0になる）
        using var binary = new Mat();
        Cv2.Threshold(gray, binary, 0, 255, ThresholdTypes.BinaryInv | ThresholdTypes.Otsu);

        // 5. ノイズ除去（小さなゴミ点を除去）
        using var kernel = Cv2.GetStructuringElement(MorphShapes.Rect, new OpenCvSharp.Size(3, 3));
        using var opened = new Mat();
        Cv2.MorphologyEx(binary, opened, MorphTypes.Open, kernel);

        // 6. 輪郭抽出
        Cv2.FindContours(opened, out PointCv[][] contours, out _, RetrievalModes.External, ContourApproximationModes.ApproxSimple);

        // 7. 小さすぎる領域は除外 (面積閾値は画像全体の0.0025%)
        int imgArea = gray.Cols * gray.Rows;
        int minArea = Math.Max((int)(imgArea * 0.000025), 10);
        var rects = new List<OpenCvSharp.Rect>();
        foreach (var cnt in contours)
        {
            var r = Cv2.BoundingRect(cnt);
            if (r.Width * r.Height >= minArea)
                rects.Add(r);
        }

        // 8. 検出結果がない場合は空領域
        if (rects.Count == 0)
            return new Rectangle(0, 0, 0, 0);

        // 9. 全領域を覆う最小矩形を計算
        int minX = rects.Min(r => r.X);
        int minY = rects.Min(r => r.Y);
        int maxX = rects.Max(r => r.X + r.Width - 1);
        int maxY = rects.Max(r => r.Y + r.Height - 1);

        return new Rectangle(minX, minY, maxX - minX + 1, maxY - minY + 1);
    }


    #endregion

    #region (D) グループ別クロップ領域決定

    // =============================  [4] ここから  =============================
    #region (D) グループ別クロップ領域決定  ★刷新★

    /// <summary>
    /// 同じグループ（奇数／偶数）に属する各ページの BoundingBox 群から、
    /// 「全ページに一律適用すべき余白除去領域」を決定する。
    /// 4 つの余白 (Left, Top, Right, Bottom) を 1 ベクトルとして扱い、
    /// ベクトル単位で外れ値 (IQR*1.5 超) を除外してから
    /// 最終的な中央値を採用する。
    /// </summary>
    private static Rectangle DecideGroupCropRegion(List<PageBoundingBox> boundingBoxes)
    {
        // ---------- 0) バリデーション ----------
        if (boundingBoxes == null || boundingBoxes.Count == 0)
            return new Rectangle(0, 0, 0, 0);

        // 面積ゼロ (真っ白ページなど) は最初から除外
        var valid = boundingBoxes
            .Where(b => b.BoundingBox.Width > 0 && b.BoundingBox.Height > 0)
            .ToList();
        if (valid.Count == 0)
            return new Rectangle(0, 0, 0, 0);

        // ---------- 1) 4 辺それぞれの統計量 ----------
        // 先に座標値を昇順に並べておく
        var lefts = valid.Select(b => b.BoundingBox.Left).OrderBy(x => x).ToList();
        var tops = valid.Select(b => b.BoundingBox.Top).OrderBy(x => x).ToList();
        var rights = valid.Select(b => b.BoundingBox.Right).OrderBy(x => x).ToList();
        var bottoms = valid.Select(b => b.BoundingBox.Bottom).OrderBy(x => x).ToList();

        // 四分位数と IQR
        int q1L = Percentile(lefts, 0.25); int q3L = Percentile(lefts, 0.75); int iqrL = q3L - q1L;
        int q1T = Percentile(tops, 0.25); int q3T = Percentile(tops, 0.75); int iqrT = q3T - q1T;
        int q1R = Percentile(rights, 0.25); int q3R = Percentile(rights, 0.75); int iqrR = q3R - q1R;
        int q1B = Percentile(bottoms, 0.25); int q3B = Percentile(bottoms, 0.75); int iqrB = q3B - q1B;

        // IQR==0 の場合に備えて “1” でガード
        if (iqrL == 0) iqrL = 1;
        if (iqrT == 0) iqrT = 1;
        if (iqrR == 0) iqrR = 1;
        if (iqrB == 0) iqrB = 1;

        // ---------- 2) ベクトル単位で外れ値ページを除外 ----------
        const double k = 1.5;   // Tukey fence
        bool IsOutlier(int v, int q1, int q3, int iqr) =>
            (v < q1 - k * iqr) || (v > q3 + k * iqr);

        var inliers = valid.Where(b =>
        {
            var bb = b.BoundingBox;
            return !(IsOutlier(bb.Left, q1L, q3L, iqrL)
                   || IsOutlier(bb.Top, q1T, q3T, iqrT)
                   || IsOutlier(bb.Right, q1R, q3R, iqrR)
                   || IsOutlier(bb.Bottom, q1B, q3B, iqrB));
        }).ToList();

        // inlier が極端に少ない場合は、「外れ値除去なし」で強制採用
        if (inliers.Count < Math.Max(3, valid.Count / 2))
            inliers = valid;

        // ---------- 3) 最終領域＝inlier の中央値 ----------
        int left = Median(inliers.Select(b => b.BoundingBox.Left).ToList());
        int top = Median(inliers.Select(b => b.BoundingBox.Top).ToList());
        int right = Median(inliers.Select(b => b.BoundingBox.Right).ToList());
        int bottom = Median(inliers.Select(b => b.BoundingBox.Bottom).ToList());

        // 幅・高さを計算
        int w = Math.Max(right - left, 0);
        int h = Math.Max(bottom - top, 0);

        // すべて真っ白ページだった等の場合の保険
        if (w == 0 || h == 0)
            return new Rectangle(0, 0, 0, 0);

        return new Rectangle(left, top, w, h);
    }

    /// <summary>
    /// 0.0–1.0 で与えた百分位を返す (線形補間)。
    /// 引数 values は必ず **昇順** にソート済みで渡すこと。
    /// </summary>
    private static int Percentile(IReadOnlyList<int> values, double p)
    {
        if (values.Count == 0) return 0;

        double idx = p * (values.Count - 1);         // 0-based
        int lo = (int)Math.Floor(idx);
        int hi = (int)Math.Ceiling(idx);
        if (lo == hi) return values[lo];

        double frac = idx - lo;
        return (int)Math.Round(values[lo] + (values[hi] - values[lo]) * frac);
    }

    #endregion
    // =============================  [4] ここまで  =============================

    private static int Median(List<int> values)
    {
        if (values.Count == 0) return 0;
        values.Sort();
        int n = values.Count;
        if (n % 2 == 1)
        {
            return values[n / 2];
        }
        else
        {
            return (values[n / 2 - 1] + values[n / 2]) / 2;
        }
    }

    private static Rectangle AddMarginAndClip(Rectangle rect, int marginPixel, int imgWidth, int imgHeight)
    {
        if (rect.Width <= 0 || rect.Height <= 0)
        {
            // 真っ白ページ等 → 全体返すor適宜処理
            return new Rectangle(0, 0, imgWidth, imgHeight);
        }

        int left = Math.Max(rect.Left - marginPixel, 0);
        int top = Math.Max(rect.Top - marginPixel, 0);
        int right = Math.Min(rect.Right + marginPixel, imgWidth - 1);
        int bottom = Math.Min(rect.Bottom + marginPixel, imgHeight - 1);

        int width = Math.Max(right - left + 1, 1);
        int height = Math.Max(bottom - top + 1, 1);

        return new Rectangle(left, top, width, height);
    }

    #endregion



    // ────────────────────────────────────────────────
    // public API
    //   src : 余白を足したい元ページ（Crop 済みでも Deskew 済みでも OK）
    //   targetW/H : 仕上げたいキャンバスサイズ
    // 戻り値     : 新しく生成した Image<Rgba32>
    // ────────────────────────────────────────────────
    public static Image<Rgba32> ResizeAndMakePaddingWithNaturalPaperColor(Image<Rgba32> src,
                                        int targetW,
                                        int targetH,
                                        int cornerPatchPercent = 3,
                                        int feather = 4,
                                        IResampler sampler = null!,
                                        int shiftX = 0,
                                        int shiftY = 0
                                        )
    {
        sampler ??= KnownResamplers.Lanczos3;

        // 1) 元ページを縦横同倍率でフィット
        double scale = Math.Min((double)targetW / src.Width,
                                (double)targetH / src.Height);
        int fittedW = (int)Math.Round(src.Width * scale);
        int fittedH = (int)Math.Round(src.Height * scale);

        using var fitted = src.Clone(ctx => ctx.Resize(fittedW, fittedH, sampler));

        int offX = (targetW - fittedW) / 2 + (int)Math.Round((double)shiftX * scale);
        int offY = (targetH - fittedH) / 2 + (int)Math.Round((double)shiftY * scale);

        // 2) 角４ヶ所から紙色を抽出
        var (cTL, cTR, cBL, cBR) =
            SampleCornerColors(fitted, cornerPatchPercent);

        // 3) グラデーション背景を描く
        var canvas = new Image<Rgba32>(targetW, targetH);
        canvas.ProcessPixelRows(bg =>
        {
            for (int y = 0; y < targetH; y++)
            {
                float vy = (float)y / (targetH - 1);
                //var top = Lerp(cTL, cTR, vy: 0, ux: 0);   // ダミー
                //   u : [0,1] ← x 位置
                //   v : [0,1] ← y 位置
                var row = bg.GetRowSpan(y);
                for (int x = 0; x < targetW; x++)
                {
                    float ux = (float)x / (targetW - 1);
                    row[x] = Bilinear(cTL, cTR, cBL, cBR, ux, vy);
                }
            }
        });

        // 4) 元画像を貼り付け
        canvas.Mutate(ctx => ctx.DrawImage(fitted,
                                    new SixLabors.ImageSharp.Point(offX, offY), 1f));

        // 5) 縫い目フェザー
        Feather(canvas, offX, offY, fittedW, fittedH, feather);

        return canvas;
    }

    /// <summary>
    /// src を縦横同倍率で Resize し、
    /// 指定された targetW×targetH のキャンバス中央から (x,y) だけシフトして貼り付け、
    /// 余白を「自然な紙色」でグラデーション補完する。
    /// x, y は <paramref name="src"/> の元座標系でのピクセル数。
    /// </summary>
    public static SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgba32>
    ResizeAndMakePaddingWithNaturalPaperColor2(
        SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgba32> src,
        int targetW,
        int targetH,
        int x,                    // ← 追加：X 方向シフト (src 座標系)
        int y,                    // ← 追加：Y 方向シフト (src 座標系)
        double scale,
        int cornerPatchPercent = 3,
        int feather = 4,
        SixLabors.ImageSharp.Processing.Processors.Transforms.IResampler sampler = null!
    )
    {
        sampler ??= SixLabors.ImageSharp.Processing.KnownResamplers.Lanczos3;

        // 1) 元ページを縦横同倍率でフィット
        int fittedW = (int)System.Math.Round(src.Width * scale);
        int fittedH = (int)System.Math.Round(src.Height * scale);

        using var fitted = src.Clone(ctx => ctx.Resize(fittedW, fittedH, sampler));

        // 2) シフト量を src → fitted のスケールで変換
        int shiftX = (int)System.Math.Round(x * scale);
        int shiftY = (int)System.Math.Round(y * scale);

        // 3) キャンバス中央に配置した後にシフトを加算
        int offX = /*(targetW - fittedW) / 2 +*/ shiftX;
        int offY = /*(targetH - fittedH) / 2 +*/ shiftY;

        // 4) 角４ヶ所から紙色を抽出
        var (cTL, cTR, cBL, cBR) = SampleCornerColors(fitted, cornerPatchPercent);

        // 5) グラデーション背景を描く
        var canvas = new SixLabors.ImageSharp.Image<SixLabors.ImageSharp.PixelFormats.Rgba32>(targetW, targetH);
        canvas.ProcessPixelRows(bg =>
        {
            for (int yy = 0; yy < targetH; yy++)
            {
                float v = (float)yy / (targetH - 1);
                var row = bg.GetRowSpan(yy);
                for (int xx = 0; xx < targetW; xx++)
                {
                    float u = (float)xx / (targetW - 1);
                    row[xx] = Bilinear(cTL, cTR, cBL, cBR, u, v);
                }
            }
        });

        // 6) 元画像を貼り付け（はみ出しは ImageSharp 側で自動クリップ）
        canvas.Mutate(ctx =>
            ctx.DrawImage(fitted,
                new SixLabors.ImageSharp.Point(offX, offY), 1f));

        // 7) 縫い目フェザー
        Feather(canvas, offX, offY, fittedW, fittedH, feather);

        return canvas;
    }


    // ── 角４色を取る ───────────────────────────────
    private static (Rgba32 tl, Rgba32 tr, Rgba32 bl, Rgba32 br)
        SampleCornerColors(Image<Rgba32> img, int percent)
    {
        int patchW = Math.Max(img.Width * percent / 100, 8);
        int patchH = Math.Max(img.Height * percent / 100, 8);

        Rgba32 tl = AveragePaperColor(img, 0, 0, patchW, patchH);
        Rgba32 tr = AveragePaperColor(img, img.Width - patchW, 0, patchW, patchH);
        Rgba32 bl = AveragePaperColor(img, 0, img.Height - patchH, patchW, patchH);
        Rgba32 br = AveragePaperColor(img, img.Width - patchW,
                                               img.Height - patchH,
                                               patchW, patchH);

        return (tl, tr, bl, br);
    }

    // ── 指定矩形の「紙だけ平均色」(EstimatePaperColor の局所版) ─────────────
    private static Rgba32 AveragePaperColor(Image<Rgba32> img,
                                            int sx, int sy,
                                            int w, int h)
    {
        // 0) 範囲を画像内にクリップ
        sx = Math.Clamp(sx, 0, img.Width - 1);
        sy = Math.Clamp(sy, 0, img.Height - 1);
        w = Math.Clamp(w, 1, img.Width - sx);
        h = Math.Clamp(h, 1, img.Height - sy);

        // 1) 輝度ヒストグラム（2px ストライドで間引き）
        Memory<int> histMem = new int[256];
        var hist = histMem.Span;
        long samples = 0;

        img.ProcessPixelRows(access =>
        {
            var hist = histMem.Span;
            for (int y = sy; y < sy + h; y += 2)
            {
                var row = access.GetRowSpan(y);
                for (int x = sx; x < sx + w; x += 2)
                {
                    var p = row[x];
                    int lum = (p.R * 299 + p.G * 587 + p.B * 114) / 1000;
                    hist[lum]++;
                    samples++;
                }
            }
        });

        if (samples == 0)           // ごく小さいパッチの場合
            return EstimatePaperColor(img);

        // 2) 上位 5 % に相当する輝度しきい値を求める
        long target = (long)(samples * 0.05);
        long acc = 0;
        int thr = 255;
        for (int i = 255; i >= 0; i--)
        {
            acc += hist[i];
            if (acc >= target) { thr = i; break; }
        }

        // しきい値が暗すぎて意味がなさそうならフォールバック
        if (thr < 150) return EstimatePaperColor(img);

        // 3) 低彩度画素だけを平均
        long sumR = 0, sumG = 0, sumB = 0, cnt = 0;

        img.ProcessPixelRows(access =>
        {
            for (int y = sy; y < sy + h; y += 2)
            {
                var row = access.GetRowSpan(y);
                for (int x = sx; x < sx + w; x += 2)
                {
                    var p = row[x];
                    int lum = (p.R * 299 + p.G * 587 + p.B * 114) / 1000;
                    if (lum < thr) continue;

                    int max = Math.Max(p.R, Math.Max(p.G, p.B));
                    int min = Math.Min(p.R, Math.Min(p.G, p.B));
                    int sat = max == 0 ? 0 : (max - min) * 255 / max;   // 0–255
                    if (sat >= 40) continue;                            // 彩度フィルタ

                    sumR += p.R; sumG += p.G; sumB += p.B; cnt++;
                }
            }
        });

        // 4) 画素が取れなかったら全体推定でフォールバック
        if (cnt == 0) return EstimatePaperColor(img);

        return new Rgba32((byte)(sumR / cnt),
                          (byte)(sumG / cnt),
                          (byte)(sumB / cnt));
    }

    // ── 双線形補間 ────────────────────────────────
    private static Rgba32 Bilinear(Rgba32 tl, Rgba32 tr,
                                   Rgba32 bl, Rgba32 br,
                                   float u, float v)
    {
        // u : 0→左, 1→右
        // v : 0→上, 1→下
        static float Lerp(float a, float b, float t) => a + (b - a) * t;
        byte r = (byte)Lerp(Lerp(tl.R, tr.R, u), Lerp(bl.R, br.R, u), v);
        byte g = (byte)Lerp(Lerp(tl.G, tr.G, u), Lerp(bl.G, br.G, u), v);
        byte b = (byte)Lerp(Lerp(tl.B, tr.B, u), Lerp(bl.B, br.B, u), v);
        return new Rgba32(r, g, b);
    }

    // ── Feather 描画 ───────────────────────────────
    private static void Feather(Image<Rgba32> canvas,
                                int offX, int offY,
                                int fittedW, int fittedH,
                                int range)
    {
        canvas.ProcessPixelRows(access =>
        {
            for (int y = offY - range; y < offY + fittedH + range; y++)
            {
                if (y < 0 || y >= canvas.Height) continue;
                var row = access.GetRowSpan(y);
                for (int x = offX - range; x < offX + fittedW + range; x++)
                {
                    if (x < 0 || x >= canvas.Width) continue;
                    bool inside = (x >= offX && x < offX + fittedW &&
                                   y >= offY && y < offY + fittedH);
                    // d = 0 (境界上) → 1 (range px 外)
                    int dx = Math.Max(0, Math.Max(offX - x, x - (offX + fittedW - 1)));
                    int dy = Math.Max(0, Math.Max(offY - y, y - (offY + fittedH - 1)));
                    int d = Math.Max(dx, dy);
                    if (d >= range) continue;            // 100 % 背景なので処理不要

                    float alpha = d / (float)range;      // 0→境界, 1→完全背景
                    var bg = row[x];                     // 既に背景色
                    var fg = inside ? canvas[offX + (x - offX), offY + (y - offY)]
                                    : bg;               // inside: 元ページ, outside: bg
                    row[x] = Lerp(bg, fg, alpha);
                }
            }
        });

        static Rgba32 Lerp(Rgba32 a, Rgba32 b, float t)
        {
            static byte L(float aa, float bb, float t) => (byte)(aa + (bb - aa) * t);
            return new Rgba32(L(a.R, b.R, t),
                              L(a.G, b.G, t),
                              L(a.B, b.B, t),
                              255);
        }
    }
}

#region 補助クラス

internal class PageInfo
{
    public string FilePath { get; set; } = null!;
    public string FilePathColorAdj = null!;
    public int PageNumber { get; set; }
    public bool IsOdd { get; set; }
}

internal class ColorStats
{
    public int PageNumber { get; set; }

    // 既存（Mean*）は「紙（背景）」平均として再利用
    public double MeanR { get; set; }
    public double MeanG { get; set; }
    public double MeanB { get; set; }

    // ★ 新規追加：背景とインクそれぞれの RGB 平均
    public double PaperR { get; set; }
    public double PaperG { get; set; }
    public double PaperB { get; set; }
    public double InkR { get; set; }
    public double InkG { get; set; }
    public double InkB { get; set; }
}

internal class GlobalColorParam
{
    public double OffsetR { get; set; }
    public double OffsetG { get; set; }
    public double OffsetB { get; set; }
    public double ScaleR { get; set; }
    public double ScaleG { get; set; }
    public double ScaleB { get; set; }

    public byte GhostSuppressLumThreshold;   // ゴースト抑制用しきい値 (Y8)
    public byte WhiteClipRange;              // 「ほぼ紙」→真っ白 に引き上げるレンジ

    public byte PaperR, PaperG, PaperB;     // 背景代表色
    public byte SatThreshold;               // 彩度閾値 (0-255)
    public byte ColorDistThreshold;         // 紙色との距離閾値 (L1)

    // ↓ 追加 ↓
    /// <summary>裏写り（オレンジ／黄色）を抑制する色相の下限（度）</summary>
    public float BleedHueMin { get; set; }
    /// <summary>裏写り（オレンジ／黄色）を抑制する色相の上限（度）</summary>
    public float BleedHueMax { get; set; }
    /// <summary>裏写り抑制を行う際の最低明度（0–1 スケール）</summary>
    public float BleedValueMin { get; set; }

}

internal class PageBoundingBox
{
    public string SrcPath { get; set; } = null!;
    public int PageNumber { get; set; }
    public Rectangle BoundingBox { get; set; }
}









/// <summary>
/// 文字候補 (Character + 可能性パーセンテージ) を表すクラス
/// </summary>
public class PnOcrCharacterPossibilitiesItem
{
    public char Character;
    public double Possibility;

    public override string ToString() => $"'{Character}' ({Possibility:F3})";

    public PnOcrCharacterPossibilitiesItem() { }

    public PnOcrCharacterPossibilitiesItem(char character, int possibilityPercentage)
    {
        this.Character = character;
        this.Possibility = (double)possibilityPercentage / 100.0;
    }
}

/// <summary>
/// 1文字分の OCR 結果 (矩形領域 + 文字候補リスト) を表すクラス
/// </summary>
public class PnOcrResultCharacter
{
    /// <summary>
    /// targetRect からの相対座標 (X,Y,Width,Height)
    /// </summary>
    public SixLabors.ImageSharp.Rectangle Rect;

    /// <summary>
    /// 可能性の高い順にソートされた CharacterPossibilitiesItem のリスト
    /// </summary>
    public List<PnOcrCharacterPossibilitiesItem> CharacterPossibilities = null!;

    public PnOcrResultCharacter() { }

    public PnOcrResultCharacter(SixLabors.ImageSharp.Rectangle rect, IEnumerable<PnOcrCharacterPossibilitiesItem> characterPossibilities)
    {
        this.Rect = rect;
        this.CharacterPossibilities = characterPossibilities.ToList();
    }

    public override string ToString() => $"'{this.CharacterPossibilities.FirstOrDefault()?.Character}' ({Rect.ToString()})";

}

public class PnOcrPossibilityWordText
{
    public string Text = "";
    public int TextInt;
    public double Possibility;
    public Rectangle BoundingBox;

    public override string ToString() => $"{Text} ({Possibility:F3})";
}

// 複数の OCR エンジンでの単語推定結果のセット
public class PnOcrResultWordSet
{
    public List<PnOcrResultWord> WordSet = null!;

    public List<PnOcrPossibilityWordText> PossibilityWords = null!;

    public bool RightSide { get; }

    public PnOcrResultWordSet() { }

    public PnOcrResultWordSet(IEnumerable<PnOcrResultWord> resultList, bool rightSide)
    {
        this.WordSet = resultList.ToList();

        this.RightSide = rightSide;

        var uniqueTextList = this.WordSet.SelectMany(x => x.PossibilityWords).Select(x => x.Text).Distinct(StrCmpi);

        List<PnOcrPossibilityWordText> plist = new();

        foreach (var uniqueText in uniqueTextList)
        {
            PnOcrPossibilityWordText? word;

            if (RightSide == false)
            {
                word = this.WordSet.SelectMany(x => x.PossibilityWords).Where(x => x.Text._IsSamei(uniqueText)).OrderBy(x => x.BoundingBox.Left).FirstOrDefault();
            }
            else
            {
                word = this.WordSet.SelectMany(x => x.PossibilityWords).Where(x => x.Text._IsSamei(uniqueText)).OrderBy(x => x.BoundingBox.Right).LastOrDefault();
            }

            if (word != null)
            {
                plist.Add(word);
            }
        }

        this.PossibilityWords = plist;
    }
}

public class PnOcrTargetRect
{
    public SixLabors.ImageSharp.Rectangle Rect;
    public PnOcrResultWordSet? WordSet;

    public PnOcrTargetRect() { }

    public PnOcrTargetRect(SixLabors.ImageSharp.Rectangle rect, PnOcrResultWordSet? wordSet)
    {
        this.Rect = rect;
        this.WordSet = wordSet;
    }
}

// 単一の OCR エンジンの単語推定結果
public class PnOcrResultWord
{
    public List<PnOcrResultCharacter> CharactersList = null!;

    public List<PnOcrPossibilityWordText> PossibilityWords = null!;

    public SixLabors.ImageSharp.Rectangle BoundingBox;

    public PnOcrResultWord() { }

    public PnOcrResultWord(IEnumerable<PnOcrResultCharacter> characters)
    {
        CharactersList = characters.ToList();

        this.BoundingBox = CharactersList.Select(x => x.Rect).GetBoundingBoxFromRectangles();

        this.PossibilityWords = GeneratePossibilityWordTexts();
    }

    public override string ToString() => $"'{this.PossibilityWords.FirstOrDefault()?.Text}' ({BoundingBox.ToString()})";

    /// <summary>
    /// OCR された単語から「あり得る文字列とその確率」の一覧を作成。
    /// 同じ文字列は確率を合算し、確率の高い順に並べる。
    /// </summary>
    List<PnOcrPossibilityWordText> GeneratePossibilityWordTexts()
    {
        // ① 文字ごとの候補集合を直積展開しながら確率を掛け算
        var raw =
            this.CharactersList                              // IEnumerable<PnOcrResultCharacter>
                .Select(c => c.CharacterPossibilities)    // IEnumerable<IEnumerable<PnOcrCharacterPossibilitiesItem>>
                .Aggregate(
                    seed: Enumerable.Repeat(              // ← ここがポイント（IEnumerable にする）
                        new { Text = "", Possibility = 1.0 },
                        1),
                    (prefixes, nextChars) =>
                        prefixes.SelectMany(prefix =>
                            nextChars.Select(item => new
                            {
                                Text = prefix.Text + item.Character,
                                Possibility = prefix.Possibility * item.Possibility
                            }))
                );

        var boundingBox = CharactersList.Select(x => x.Rect).GetBoundingBoxFromRectangles();

        // ② 同一文字列を合併して確率を合算 → 降順ソート
        return raw
            .GroupBy(x => x.Text)
            .Select(g => new PnOcrPossibilityWordText
            {
                Text = g.Key,
                TextInt = g.Key._ToInt(),
                Possibility = g.Sum(x => x.Possibility),
                BoundingBox = boundingBox,
            })
            .OrderByDescending(p => p.Possibility)
            .ToList();
    }

}

[Flags]
public enum PnOcrEngineType
{
    Type0_EnglishNumbersOnly = 0,
    Type1_JapaneseGeneric
}

public class PnOcrPageNumberMatchEntry
{
    public SixLabors.ImageSharp.Rectangle Rect;
    public string Text = "";
    public int TextInt;
    public double Possibility;
    public SixLabors.ImageSharp.Rectangle BoundingBox;
}

public class PnOcrLibPageMetaData
{
    public string FilePath = "";
    public List<PnOcrTargetRect> RectSet = new();
    public int PhysicalFileNumber; // 0 から始まる物理的ページ番号
    public int LogicalPageNumber;
    public bool IsVerticalWriting;
    public double IsVerticalWriting_Probability;

    // 当該ページに登場する OCR 結果のページ番号一覧表
    public Dictionary<int, double> OcrPossiblePageIntScoreList = new();
    public Dictionary<int, List<PnOcrPageNumberMatchEntry>> EntryListByPageInt = new();
    public List<PnOcrPageNumberMatchEntry> AllEntryList = new();

    public PnOcrPageNumberMatchEntry? FoundPageNumberEntry;
    public SixLabors.ImageSharp.Point? PageNumberStdPoint;
    public int Shift_X, Shift_Y;
}

public class PnOcrLibBookMetaData
{
    public List<PnOcrLibPageMetaData> Pages = new();

    public bool IsVerticalWriting;
    public double IsVerticalWriting_Probability;
}

public static class PnOcrLib
{

    public static async Task<PnOcrLibBookMetaData?> OcrProcessForBookAsync(IEnumerable<string> pngFilePathList, bool saveDebugPng = false)
    {
        PnOcrLibBookMetaData book = new();

        var filePathList = pngFilePathList.ToList().OrderBy(x => x, StrCmpi)._ToListWithIndex();

        ConcurrentBag<PnOcrLibPageMetaData> pageMetaDataList = new();

        await filePathList._DoForEachParallelAsync(async (item, threadId) =>
        {
            await Task.Yield();

            Con.WriteLine($"OcrProcessForBookAsync: Doing OCR: {PP.GetFileName(item.Value)}");

            string debugFilePath1_Ocr = "";
            string debugFilePath2_Otsu = "";

            if (saveDebugPng)
            {
                string dirPath = PP.GetDirectoryName(item.Value);
                string subDirPath = PP.Combine(dirPath, "_debug1_ocr");
                debugFilePath1_Ocr = PP.Combine(subDirPath, PP.GetFileNameWithoutExtension(item.Value) + ".png");
            }

            if (saveDebugPng)
            {
                string dirPath = PP.GetDirectoryName(item.Value);
                string subDirPath = PP.Combine(dirPath, "_debug2_otsu");
                debugFilePath2_Otsu = PP.Combine(subDirPath, PP.GetFileNameWithoutExtension(item.Value) + ".png");
            }

            using var srcImage = await SixLabors.ImageSharp.Image.LoadAsync<Rgb24>(item.Value);

            var pageMetaData = await OcrDetectPageNumberCandidatesAsync(srcImage, threadId, debugPngFilePath1_Ocr: debugFilePath1_Ocr, debugPngFilePath2_Otsu: debugFilePath2_Otsu);

            pageMetaData.FilePath = item.Value;
            pageMetaData.PhysicalFileNumber = item.Key;

            pageMetaDataList.Add(pageMetaData);
        },
        Env.NumCpus);

        book.Pages = pageMetaDataList.ToList().OrderBy(x => x.PhysicalFileNumber).ToList();

        Con.WriteLine($"OcrProcessForBookAsync: Internal processing");

        // 縦書きかどうか
        if (book.Pages.Count >= 10)
        {
            book.IsVerticalWriting_Probability = book.Pages.Average(x => x.IsVerticalWriting_Probability); ;
        }

        book.IsVerticalWriting = book.IsVerticalWriting_Probability >= 0.5;

        // 前処理
        foreach (var page in book.Pages)
        {
            page.OcrPossiblePageIntScoreList = new();
            page.EntryListByPageInt = new();

            HashSetDictionary<int, double> tmpDict = new();

            foreach (var rect in page.RectSet)
            {
                if (rect.WordSet != null)
                {
                    var wordset = rect.WordSet;

                    foreach (var possibleWord in wordset!.PossibilityWords)
                    {
                        tmpDict.Add(possibleWord.TextInt, possibleWord.Possibility);

                        PnOcrPageNumberMatchEntry entry = new PnOcrPageNumberMatchEntry()
                        {
                            BoundingBox = rect.Rect.GetAbsoluteRectFromRelativeChildRect(possibleWord.BoundingBox)!.Value,
                            Possibility = possibleWord.Possibility,
                            Text = possibleWord.Text,
                            TextInt = possibleWord.TextInt,
                            Rect = rect.Rect,
                        };

                        page.AllEntryList.Add(entry);
                        page.EntryListByPageInt._GetOrNew(possibleWord.TextInt, () => new()).Add(entry);
                    }
                }
                else
                {
                    // OCR 失敗しているが Rect として検出されているもの
                    PnOcrPageNumberMatchEntry entry = new PnOcrPageNumberMatchEntry()
                    {
                        BoundingBox = rect.Rect,
                        Possibility = 0.0,
                        Text = "",
                        TextInt = 0,
                        Rect = rect.Rect,
                    };
                    page.AllEntryList.Add(entry);
                }
            }

            foreach (var key in tmpDict.Keys)
            {
                page.OcrPossiblePageIntScoreList.Add(key, tmpDict[key].Average());
            }
        }

        SortedDictionary<int, PnOcrLibPageMetaData> group0 = new();
        SortedDictionary<int, PnOcrLibPageMetaData> group1 = new();
        List<SortedDictionary<int, PnOcrLibPageMetaData>> groupsList = new();
        groupsList.Add(group0);
        groupsList.Add(group1);

        // 物理ページ番号の奇数 / 偶数で 2 グループ化する
        foreach (var page in book.Pages)
        {
            SortedDictionary<int, PnOcrLibPageMetaData> tmp;

            if ((page.PhysicalFileNumber % 2) == 0)
            {
                tmp = group0;
            }
            else
            {
                tmp = group1;
            }

            tmp.Add(page.PhysicalFileNumber, page);
        }

        // 各グループごとに、絶対ページ番号をずらして探索し、OCR 結果のページ番号候補と最も一致率の高いものを 1 つ選ぶ
        SortedDictionary<int, double> shiftScoreRank = new();
        SortedDictionary<int, int> numMatchedByShift = new();
        foreach (var group in groupsList)
        {
            for (int shiftTest = -300; shiftTest < 300; shiftTest++)
            {
                double possibilitySum = 0;

                foreach (var page in group.Values)
                {
                    int kariPage = page.PhysicalFileNumber - shiftTest;

                    if (kariPage >= 1)
                    {
                        if (page.OcrPossiblePageIntScoreList.ContainsKey(kariPage))
                        {
                            possibilitySum += page.OcrPossiblePageIntScoreList[kariPage];

                            numMatchedByShift[shiftTest] = numMatchedByShift.GetValueOrDefault(shiftTest) + 1;
                        }
                    }
                }

                double current = shiftScoreRank._GetOrNew(shiftTest, () => 0.0);
                current += possibilitySum;
                shiftScoreRank[shiftTest] = current;
            }
        }

        // シフト分の確定
        int shift = shiftScoreRank.Where(x => x.Value._IsNearlyZero() == false).OrderByDescending(x => x.Value).ThenBy(x => x.Key).Select(x => Math.Abs(x.Key)).FirstOrDefault(0);
        int numMatch = numMatchedByShift.GetValueOrDefault(shift, 0);

        // 合致するページ数が、全体の 1/3 未満である場合、あるいは 5 ページ未満の場合は、精度が十分でないので、何もしない
        if (numMatch < 5 || (numMatch * 3) < book.Pages.Max(x => x.PhysicalFileNumber))
        {
            return book;
        }

        // 縦横ズレを許容するマージンサイズ (px)
        int marginSizeWidth = (int)((double)SuperPdfUtil.internalHighResImgHeight * 0.030); // ひとまず 3.0% まで許容
        int marginSizeHeight = (int)((double)SuperPdfUtil.internalHighResImgHeight * 0.025); // ひとまず 2.5% まで許容

        Rectangle[] pageNumberBasicRectWithMargin = new Rectangle[2];
        //Rectangle[] pageNumberBasicRectOriginal = new Rectangle[2];

        // 奇数／偶数グループごとに、できるだけ多くのページにおいて、ページ番号が格納されているものとみなすことに適したバウンディングボックスの最良のものを選出する
        for (int groupNumber = 0; groupNumber < 2; groupNumber++)
        {
            var group = groupsList[groupNumber];

            // まず、一致があるページごとに、一致する OCR 結果の文字部分のバウンディングボックスを列挙する
            List<CalcBBoxScoreItem> boundingBoxCandidatesList = new();
            foreach (var page in group.Values)
            {
                int pageNumber = page.PhysicalFileNumber - shift;

                if (pageNumber >= 1)
                {
                    if (page.EntryListByPageInt.TryGetValue(pageNumber, out var matchList))
                    {
                        foreach (var match in matchList)
                        {
                            var bbox = new Rectangle(match.BoundingBox.X - marginSizeWidth, match.BoundingBox.Y - marginSizeHeight, match.BoundingBox.Width + marginSizeWidth * 2, match.BoundingBox.Height + marginSizeHeight * 2);
                            boundingBoxCandidatesList.Add(new CalcBBoxScoreItem { BoundingBoxWithMargin = bbox, BoundingBoxOriginal = match.BoundingBox, NumDigits = match.Text.Length });
                        }
                    }
                }
            }

            // 列挙された各バウンディングボックス (上下左右にスキャンズレマージンを加味) ごとに、全ページにおいて包含されるページ番号件数をカウントする
            foreach (var candidate in boundingBoxCandidatesList)
            {
                foreach (var page in group.Values)
                {
                    int pageNumber = page.PhysicalFileNumber - shift;

                    if (pageNumber >= 1)
                    {
                        if (page.EntryListByPageInt.TryGetValue(pageNumber, out var matchList))
                        {
                            foreach (var match in matchList)
                            {
                                if (candidate.BoundingBoxWithMargin.RectContainsOrExactSameToRect(match.BoundingBox))
                                {
                                    candidate.NumMatches++;
                                }
                            }
                        }
                    }
                }
            }

            //boundingBoxCandidatesList.OrderByDescending(x => x.NumMatches)._PrintAsJson();

            // 最大 match 数の 70% を超えるもののうち、面積が小さい順に並べて、上位 30% を列挙する
            int maxNumMatch = (int)((double)boundingBoxCandidatesList.Max(x => x.NumMatches) * 0.7);
            //int numTake = Math.Max(1, (int)((double)boundingBoxCandidatesList.Count * 0.3));
            var bbList = boundingBoxCandidatesList.Where(x => x.NumMatches >= maxNumMatch).OrderBy(x => x.BoundingBoxWithMargin.Width * x.BoundingBoxWithMargin.Height).TakeUntilPosition(0.3);

            // この上位 30% を全部重ねて、最も重なり合いが多い領域を検索し、その結果から最も一般的な中心点を計算する
            var centerPoint = bbList.Select(x => x.BoundingBoxWithMargin).CalcMostOverlapRect().GetRectCenterPoint();

            // 最大 match 数の 70% を超えるもののうち、Width が小さい順に並べて、真ん中のものを取得する
            int widthWithMargin = boundingBoxCandidatesList.Where(x => x.NumMatches >= maxNumMatch).OrderBy(x => x.BoundingBoxWithMargin.Width).ElementAtPosition(0.5).BoundingBoxWithMargin.Width;
            int heightWithMargin = boundingBoxCandidatesList.Where(x => x.NumMatches >= maxNumMatch).OrderBy(x => x.BoundingBoxWithMargin.Height).ElementAtPosition(0.5).BoundingBoxWithMargin.Height;

            Rectangle bbWithMargin = new Rectangle(centerPoint.X - widthWithMargin / 2, centerPoint.Y - heightWithMargin / 2, widthWithMargin, heightWithMargin);
            pageNumberBasicRectWithMargin[groupNumber] = bbWithMargin;


            //// 最大 match 数の 70% を超えるもののうち、面積が小さい順に並べて、1 / 3 のものを標準とする
            //var standard = boundingBoxCandidatesList.Where(x => x.NumMatches >= maxNumMatch).OrderBy(x => x.BoundingBoxOriginal.Width * x.BoundingBoxOriginal.Height).ElementAt(boundingBoxCandidatesList.Count / 3);
            //pageNumberBasicRectOriginal[groupNumber] = standard.BoundingBoxOriginal;
        }

        bool isGroup0RightSide = false;

        if (pageNumberBasicRectWithMargin[0].Left > pageNumberBasicRectWithMargin[1].Right)
        {
            // グループ 0 が右側、グループ 1 が左側
            isGroup0RightSide = true;
        }

        isGroup0RightSide._Print();

        // 奇数／偶数グループごとに、すべてのページに対して、BasicRect からみて最も最適なページ番号部分を検索する
        for (int groupNumber = 0; groupNumber < 2; groupNumber++)
        {
            var group = groupsList[groupNumber];
            bool isRightSide = groupNumber == 0 ? isGroup0RightSide : !isGroup0RightSide;
            var basicBoundingBox = pageNumberBasicRectWithMargin[groupNumber];

            var basicCenterPoint = basicBoundingBox.GetRectCenterPoint();

            //// 基準位置
            //var standardOriginalRect = pageNumberBasicRectOriginal[groupNumber];
            //SixLabors.ImageSharp.Point basicShiftPoint;
            //if (isRightSide == false)
            //{
            //    basicShiftPoint = new ImagePoint(standardOriginalRect.Left, (standardOriginalRect.Top + standardOriginalRect.Bottom) / 2);
            //}
            //else
            //{
            //    basicShiftPoint = new ImagePoint(standardOriginalRect.Right, (standardOriginalRect.Top + standardOriginalRect.Bottom) / 2);
            //}

            foreach (var page in group.Values)
            {
                PnOcrPageNumberMatchEntry? foundPageNumber = null;

                int pageNumber = page.PhysicalFileNumber - shift;

                //$"--- page {pageNumber}"._Print();

                if (pageNumber >= 1)
                {
                    page.LogicalPageNumber = pageNumber;

                    // まず、ページ番号が完全一致するもので、領域内に収まるものがあれば、ズレが最小のものを選択する
                    if (page.EntryListByPageInt.TryGetValue(pageNumber, out var matchList))
                    {
                        foundPageNumber = matchList.Where(x => basicBoundingBox.RectContainsOrExactSameToRect(x.BoundingBox)).OrderBy(x => basicCenterPoint.CalcDistance(x.BoundingBox.GetRectCenterPoint())).ThenBy(x => x.BoundingBox.Width * x.BoundingBox.Height).FirstOrDefault();
                    }

                    if (foundPageNumber == null)
                    {
                        // ページ番号が完全に一致しない場合で、OCR に成功しており、領域内に収まるものがあれば、OCR 結果で類似度が最大で、かつ、ズレが最小のものを選択する
                        string pageNumberStr = pageNumber.ToString();
                        foundPageNumber = page.AllEntryList.Where(x => x.Text._IsFilled() && basicBoundingBox.RectContainsOrExactSameToRect(x.BoundingBox)).OrderByDescending(x => x.Text._GetTwoStringSimilarity(pageNumberStr)).ThenBy(x => basicCenterPoint.CalcDistance(x.BoundingBox.GetRectCenterPoint())).ThenBy(x => x.BoundingBox.Width * x.BoundingBox.Height).FirstOrDefault();
                    }
                }

                // それでも見つからなければ、文字っぽいもの (一応 OCR は成功している) で、領域内に収まるものがあれば、ズレが最小のものを選択する
                if (foundPageNumber == null)
                {
                    foundPageNumber = page.AllEntryList.Where(x => x.Text._IsFilled() && basicBoundingBox.RectContainsOrExactSameToRect(x.BoundingBox)).OrderBy(x => basicCenterPoint.CalcDistance(x.BoundingBox.GetRectCenterPoint())).ThenBy(x => x.BoundingBox.Width * x.BoundingBox.Height).FirstOrDefault();
                }

                // それでも見つからなければ、OCR 成功領域以外でもよいので、領域内に収まるものがあれば、ズレが最小のものを選択する
                if (foundPageNumber == null)
                {
                    foundPageNumber = page.AllEntryList.Where(x => basicBoundingBox.RectContainsOrExactSameToRect(x.BoundingBox)).OrderBy(x => basicCenterPoint.CalcDistance(x.BoundingBox.GetRectCenterPoint())).ThenBy(x => x.BoundingBox.Width * x.BoundingBox.Height).FirstOrDefault();
                }

                //foundPageNumber._PrintAsJson();

                page.FoundPageNumberEntry = foundPageNumber;

                if (foundPageNumber != null)
                {
                    // このページにおけるページ番号部分の基準位置
                    var thisRect = foundPageNumber.Rect;
                    SixLabors.ImageSharp.Point thisStdPoint;
                    if (isRightSide == false)
                    {
                        thisStdPoint = new ImagePoint(thisRect.Left, (thisRect.Top + thisRect.Bottom) / 2);
                    }
                    else
                    {
                        thisStdPoint = new ImagePoint(thisRect.Right, (thisRect.Top + thisRect.Bottom) / 2);
                    }

                    page.PageNumberStdPoint = thisStdPoint;
                }
            }
        }

        // 奇数／偶数グループのそれぞれについて、PageNumberStdPoint の Y 平均値を計算
        int averageY_Group0 = (int)groupsList[0].Where(x => x.Value.PageNumberStdPoint != null).Select(x => x.Value.PageNumberStdPoint!.Value.Y).Average();
        int averageY_Group1 = (int)groupsList[1].Where(x => x.Value.PageNumberStdPoint != null).Select(x => x.Value.PageNumberStdPoint!.Value.Y).Average();

        int zure = Math.Abs(averageY_Group0 - averageY_Group1);

        int additionalShiftY_Group0 = 0;
        int additionalShiftY_Group1 = 0;

        Con.WriteLine($"averageY_Group0 = {averageY_Group0}, averageY_Group1 = {averageY_Group1}, zure = {zure}");

        if (zure < (marginSizeHeight * 2.0))
        {
            // 奇数／偶数グループ間の Y 基準値のズレが許容マージン範囲内であれば、奇数／偶数グループ間での基準 Y の値をぴったり合わせる
            int averageY_Both = (averageY_Group0 + averageY_Group1) / 2;

            additionalShiftY_Group0 = averageY_Both - averageY_Group0;
            additionalShiftY_Group1 = averageY_Both - averageY_Group1;

            Con.WriteLine($"averageY_Both = {averageY_Both}, additionalShiftY_Group0 = {additionalShiftY_Group0}, additionalShiftY_Group1 = {additionalShiftY_Group1}");
        }

        for (int groupNumber = 0; groupNumber < 2; groupNumber++)
        {
            var group = groupsList[groupNumber];

            // 奇数／偶数グループごとに、すぺてのページに対して、PageNumberStdPoint の平均値を計算する
            int averageX = (int)group.Where(x => x.Value.PageNumberStdPoint != null).Select(x => x.Value.PageNumberStdPoint!.Value.X).Average();

            //int averageY = (int)group.Where(x => x.Value.PageNumberStdPoint != null).Select(x => x.Value.PageNumberStdPoint!.Value.Y).Average();
            int averageY = (groupNumber == 0) ? averageY_Group0 : averageY_Group1;

            int additionalShiftY = (groupNumber == 0) ? additionalShiftY_Group0 : additionalShiftY_Group1;

            // すぺてのページのシフト量を書き込む
            foreach (var page in group.Values)
            {
                if (page.PageNumberStdPoint != null)
                {
                    int shift_x = averageX - page.PageNumberStdPoint.Value.X;
                    int shift_y = averageY - page.PageNumberStdPoint.Value.Y;

                    shift_y += additionalShiftY;

                    page.Shift_X = shift_x;
                    page.Shift_Y = shift_y;
                }

                //Con.WriteLine($"Page {page.PhysicalFileNumber + 1} ({page.LogicalPageNumber})  x = {page.Shift_X}, y = {page.Shift_Y}");
            }
        }

        return book;
    }

    public class CalcBBoxScoreItem
    {
        public Rectangle BoundingBoxWithMargin;
        public Rectangle BoundingBoxOriginal;

        public int NumDigits;

        public int NumMatches;
    }

    // ページ番号の可能性がある部分を列挙し OCR 読み取りも一応試行する
    public static async Task<PnOcrLibPageMetaData> OcrDetectPageNumberCandidatesAsync(SixLabors.ImageSharp.Image<Rgb24> srcImage, int ocrEngineInstanceIndex = 0,
        double ignorePercentageWidth = 17,
        double ignorePercentageHeight = 17,
        string? debugPngFilePath1_Ocr = null,
        string? debugPngFilePath2_Otsu = null)
    {
        ignorePercentageWidth = ignorePercentageWidth / 100.0;
        ignorePercentageHeight = ignorePercentageHeight / 100.0;

        using var mat = OcrPreProcessImgForPageNumbers(srcImage);

        if (debugPngFilePath2_Otsu._IsFilled())
        {
            using var dbgImg = SuperImgUtil.MatToImageSharpL8(mat);

            await Lfs.EnsureCreateDirectoryForFileAsync(debugPngFilePath2_Otsu);
            await dbgImg.SaveAsPngAsync(debugPngFilePath2_Otsu);
        }

        // 縦書きかどうかの判定
        double isVerticalWriting_Probability = IsPaperVerticalWriting_GetProbability(mat);

        PnOcrLibPageMetaData ret = new();

        ret.IsVerticalWriting_Probability = isVerticalWriting_Probability;
        ret.IsVerticalWriting = isVerticalWriting_Probability >= 0.5;

        Con.WriteLine($"isVerticalWriting = {ret.IsVerticalWriting}");

        using Image<L8> ocrTargetPaper = SuperImgUtil.MatToImageSharpL8(mat);

        Rectangle ignoreRegion;

        if (ignorePercentageWidth <= 0.0001 || ignorePercentageHeight <= 0.0001)
        {
            ignoreRegion = new(srcImage.Width / 2, srcImage.Height / 2, 1, 1);
        }
        else
        {
            ignoreRegion = new Rectangle(
                (int)((double)srcImage.Width * ignorePercentageWidth),
                (int)((double)srcImage.Height * ignorePercentageHeight),
                (int)((double)srcImage.Width * (1.0 - ignorePercentageWidth * 2)),
                (int)((double)srcImage.Height * (1.0 - ignorePercentageHeight * 2))
                );
        }

        var ocrTargetRects = OcrGetWordBlocks(mat, ignoreRegion);

        int paperCenterX = srcImage.Width / 2;

        foreach (var ocrTargetRect in ocrTargetRects)
        {
            bool isRightSide = false;

            if (((ocrTargetRect.Right + ocrTargetRect.Left) / 2) >= paperCenterX)
            {
                isRightSide = true;
            }

            try
            {
                var orcResultWord = await PerformPageNumberOcrAsync(ocrEngineInstanceIndex, ocrTargetPaper, ocrTargetRect, isRightSide);

                PnOcrTargetRect rectMeta = new PnOcrTargetRect(ocrTargetRect, orcResultWord);

                ret.RectSet.Add(rectMeta);
            }
            catch (Exception ex)
            {
                ex._Error();
            }
        }

        if (debugPngFilePath1_Ocr._IsFilled())
        {
            // デバッグ用画像
            using Image<Rgb24> debugImg = SuperImgUtil.ImageL8ToRgb24(ocrTargetPaper);
            foreach (var ocrTargetRect in ocrTargetRects)
            {
                debugImg.DrawRect(ocrTargetRect, 10, Color.Red);
            }


            foreach (var rect in ret.RectSet)
            {
                var orcResultWord = rect.WordSet;

                if (orcResultWord != null)
                {
                    var randomColor = DnImageSharpHelper.GenerateRandomGoodColor(Color.Red);

                    foreach (var word in orcResultWord.WordSet)
                    {
                        foreach (var b2 in word.CharactersList)
                        {
                            debugImg.DrawRect(rect.Rect.GetAbsoluteRectFromRelativeChildRect(b2.Rect)!.Value, 8, randomColor);
                        }
                    }

                    var wordBoundingBox = rect.Rect.GetAbsoluteRectFromRelativeChildRect(orcResultWord.WordSet.Select(x => x.BoundingBox).GetBoundingBoxFromRectangles())!.Value;

                    string debugTxt = orcResultWord.PossibilityWords.Select(x => x.Text)._Combine(" / ");

                    //"'" + orcResultWord.PossibilityWords.First().Text + "'";

                    debugImg.DrawSingleLineText(debugTxt, (wordBoundingBox.Left + wordBoundingBox.Right) / 2, wordBoundingBox.Top - 20,
                         DrawTextHorizonalAlign.Center, DrawTextVerticalAlign.Bottom,
                         60, "Consolas", true, randomColor);
                }
            }

            await Lfs.EnsureCreateDirectoryForFileAsync(debugPngFilePath1_Ocr);
            await debugImg.SaveAsPngAsync(debugPngFilePath1_Ocr);
        }

        return ret;
    }

    // 大津アルゴリズムによる書籍ページの二値化
    public static Image<Rgb24> PerformOtsuForPaperPage<TPixel>(Image<TPixel> srcImage)
        where TPixel : unmanaged, IPixel<TPixel>
    {
        // 任意のピクセル形式 → Rgb24 へ統一
        using var copyOfImage = srcImage.CloneAs<Rgb24>();

        // ② 前処理: 平滑化 → グレースケール → 二値化
        copyOfImage.Mutate(ctx =>
        {
            ctx /* .GaussianBlur(0.5f) */   // 必要ならノイズ除去
               .Contrast(1.5f)
               .Grayscale();                // グレースケール化
        });

        // ImageSharp → OpenCV Mat
        using var mat = SuperImgUtil.ImageSharpToMat(copyOfImage);
        Cv2.CvtColor(mat, mat, ColorConversionCodes.BGR2GRAY);      // 念のため再度 GRAY

        // --- 大津法 ---
        Cv2.Threshold(
            mat, mat,
            0, 255,
            ThresholdTypes.Binary | ThresholdTypes.Otsu);

        // 1ch → 3ch へ (GRAY → RGB)
        using var rgbMat = new Mat();
        Cv2.CvtColor(mat, rgbMat, ColorConversionCodes.GRAY2RGB);

        // Mat → バイト配列
        static byte[] ToBytes(Mat m)
        {
            var bytes = new byte[m.Rows * m.Cols * m.ElemSize()];
            Marshal.Copy(m.Data, bytes, 0, bytes.Length);
            return bytes;
        }

        byte[] pixelBytes = rgbMat.IsContinuous()
            ? ToBytes(rgbMat)
            : ToBytes(rgbMat.Clone());      // 非連続なら Clone で連続化

        // バイト列を ImageSharp.Image<Rgb24> としてロード
        return Image.LoadPixelData<Rgb24>(pixelBytes, rgbMat.Width, rgbMat.Height);
    }

    // フルカラー画像を、OCR ページ番号検出に適した形に二値化し、不要な飾りオブジェクト (■ や ● など) を削除する
    public static Mat OcrPreProcessImgForPageNumbers(SixLabors.ImageSharp.Image<Rgb24> srcImage)
    {
        using var copyOfImage = srcImage.Clone();

        // ② 前処理: 平滑化 → グレースケール → 二値化
        copyOfImage.Mutate(ctx =>
        {
            ctx//.GaussianBlur(0.5f)      // ノイズ除去
                .Contrast(1.5f)
               .Grayscale();           // グレースケール化
        });

        var mat = SuperImgUtil.ImageSharpToMat(copyOfImage);                         // ImageSharp→Mat 変換
        //Cv2.GaussianBlur(mat, mat, new OpenCvSharp.Size(3, 3), 0);           // 追いノイズ除去
        Cv2.CvtColor(mat, mat, ColorConversionCodes.BGR2GRAY);  // グレースケール（念のため）

        // --- 大津法による自動閾値二値化 ---
        Cv2.Threshold(
            mat,    // src
            mat,    // dst
            0,      // thresh は 0 にしておくと Otsu によって最適値が自動算出される
            255,    // maxValue
            ThresholdTypes.Binary | ThresholdTypes.Otsu
        );

        // ★ ここから  ───────────────────────────────────────────────
        // 画像内の「●」「■」のような塗りつぶされた幾何学形を除去する
        if (true)
        {
            // 1. 前景を白，背景を黒へ反転
            using var inv = new Mat();
            Cv2.BitwiseNot(mat, inv);

            // 2. 輪郭抽出  (外輪郭と穴を両方取得)
            Cv2.FindContours(
                inv,
                out PointCv[][] contours,
                out HierarchyIndex[] hier,
                RetrievalModes.CComp,              // ★ External → CComp に変更
                ContourApproximationModes.ApproxSimple);

            // 3. 走査
            for (int i = 0; i < contours.Length; i++)
            {
                // 親を持たない＝最外輪郭のみ処理
                if (hier[i].Parent != -1) continue;

                var outer = contours[i];
                var rect = Cv2.BoundingRect(outer);

                double w = rect.Width, h = rect.Height;
                if (w < 5 || h < 5) continue;      // 小ノイズ除外

                // --- (A) 外輪郭面積
                double areaOuter = Cv2.ContourArea(outer);

                // --- (B) 穴(子)の面積を引く
                double areaHoleSum = 0;
                for (int child = hier[i].Child;
                     child != -1;
                     child = hier[child].Next)
                {
                    areaHoleSum += Cv2.ContourArea(contours[child]);
                }

                double fillArea = areaOuter - areaHoleSum;   // 正味の塗り面積
                double areaRect = w * h;
                double extent = fillArea / (areaRect + 1e-5);
                double aspect = w / h;

                // 判定: 塗りつぶし率高 & 縦横比 ≈1
                if (extent >= 0.70 && aspect >= 0.75 && aspect <= 1.25)
                {
                    // 外輪郭 + その穴も含めて白塗り (穴も塗るため FloodFill が簡単)
                    var seed = new PointCv(rect.X + rect.Width / 2,
                                           rect.Y + rect.Height / 2);
                    Cv2.FloodFill(mat, seed, new Scalar(255),
                                  out _, new Scalar(0), new Scalar(0),
                                  FloodFillFlags.Link8);
                }
            }
        }


        if (true)
        {
            // ★ ここから  ───────────────────────────────────────────────
            // 1. 'mat' (黒文字, 白背景) → 反転コピー (白前景) を作る
            using var inv = new Mat();
            Cv2.BitwiseNot(mat, inv);            // inv: ■ と文字が白

            // 2. 距離変換 (L2, maskSize=5) で「画素の厚み」を取得
            using var dist = new Mat();
            Cv2.DistanceTransform(inv, dist, DistanceTypes.L2, DistanceTransformMasks.Mask5);

            // 3. “厚み ≥ 3px” の部分だけを抽出
            using var thickMask = new Mat();
            Cv2.Threshold(dist, thickMask, 3.0, 255, ThresholdTypes.Binary);   // 距離は float
            thickMask.ConvertTo(thickMask, MatType.CV_8U);                     // 輪郭検出用に 8bit へ

            // 4. 輪郭抽出
            Cv2.FindContours(
                thickMask,
                out PointCv[][] contours,
                out _,
                RetrievalModes.External,
                ContourApproximationModes.ApproxSimple);

            foreach (var cnt in contours)
            {
                var rect = Cv2.BoundingRect(cnt);
                int w = rect.Width;
                int h = rect.Height;

                // --- サイズと形でフィルタ ----------------------------------
                if (w < 10 || h < 10) continue;            // 小さ過ぎるものはノイズ
                double aspect = (double)w / h;             // 正方形なら 0.8‒1.25 に収まる
                if (aspect < 0.80 || aspect > 1.25) continue;

                double areaCnt = Cv2.ContourArea(cnt);
                double fillRate = areaCnt / (w * h + 1e-5); // 塗りつぶし率
                if (fillRate < 0.75) continue;             // 中抜き文字を除外

                // ---- ■ と判断したら元画像 'mat' を白塗り --------------------
                // 従来: Cv2.Rectangle(mat, rect, new Scalar(255), -1);

                // 1. 種点 (中心) を計算
                var seed = new PointCv(rect.X + rect.Width / 2,
                                       rect.Y + rect.Height / 2);

                // 2. Flood-Fill で seed と同じ色 (黒=0) と連結した領域を 255 で塗る
                Cv2.FloodFill(
                    image: mat,                     // OCR 用二値画像 (黒文字・白背景)
                    seedPoint: seed,
                    newVal: new Scalar(255),         // 白で塗りつぶし
                    out _,                           // 塗りつぶし結果の外接矩形は不要
                    loDiff: new Scalar(0),           // 完全一致 (黒のみ)
                    upDiff: new Scalar(0),
                    flags: FloodFillFlags.Link8);   // 8近傍
            }
            // ★ ここまで  ───────────────────────────────────────────────

        }

        return mat;
    }

    readonly static Singleton<string, TesseractEngine> OcrEngineCache = new(typeStr =>
    {
        var tokens = typeStr._Split(StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries, ":");
        var type = PnOcrEngineType.Type0_EnglishNumbersOnly.ParseAsDefault(tokens[0]);
        int index = tokens[1]._ToInt();

        return LoadOcrEngineInternal(type);
    });

    public static TesseractEngine GetOcrEngine(PnOcrEngineType type, int id)
    {
        return OcrEngineCache.CreateOrGet($"{type}:{id}");
    }

    static TesseractEngine LoadOcrEngineInternal(PnOcrEngineType type)
    {
        var eng = new TesseractEngine(SuperBookExternalTools.Settings.AiTest_TesseractOCR_Data_Dir, type == PnOcrEngineType.Type0_EnglishNumbersOnly ? "eng" : "jpn", EngineMode.LstmOnly);

        if (type == PnOcrEngineType.Type0_EnglishNumbersOnly)
        {
            // 数字のみ認識するホワイトリスト
            eng.SetVariable("tessedit_char_whitelist", "0123456789");
            // 数字認識モードを有効化
            eng.SetVariable("classify_bln_numeric_mode", "1");
        }

        // LSTM のビームサーチ選択モード（文字毎の候補取得）
        eng.SetVariable("lstm_choice_mode", "2");
        // ビームサーチ反復回数
        eng.SetVariable("lstm_choice_iterations", "5");

        //eng.SetVariable("textord_blshift_maxshift", "50");
        //eng.SetVariable("textord_blshift_xfraction", "0.7");

        //eng.SetVariable("user_defined_dpi", "600");

        //eng.SetVariable("textord_words_definite_spread", "1");
        //eng.SetVariable("textord_words_initial_lower", "0.1");

        // ページ分割モード: 水平行内の文字列として処理
        eng.DefaultPageSegMode = PageSegMode.SingleWord;
        return eng;
    }

    public static async Task<PnOcrResultWordSet?> PerformPageNumberOcrAsync(
        int ocrEngineInstanceIndex,
        Image<L8> targetImage,
        SixLabors.ImageSharp.Rectangle targetRect,
        bool rightSide)
    {
        List<PnOcrEngineType> typeList = new();

        typeList.Add(PnOcrEngineType.Type0_EnglishNumbersOnly);
        typeList.Add(PnOcrEngineType.Type1_JapaneseGeneric);

        ConcurrentBag<List<PnOcrResultWord>> tmp = new();

        await typeList._DoForEachParallelAsync(async (engType, id) =>
        {
            await Task.Yield();
            List<PnOcrResultWord> ret = PerformPageNumberOcrInternal(engType, id, targetImage, targetRect, rightSide);
            tmp.Add(ret);
        }, 2);

        var allOcrResults = tmp.ToList();

        PnOcrResultWord? primaryBlock; // 最も相応しい文字列ブロック

        if (rightSide)
        {
            primaryBlock = allOcrResults.SelectMany(x => x).OrderByDescending(x => x.CharactersList.Max(x => x.Rect.Right)).FirstOrDefault();
        }
        else
        {
            primaryBlock = allOcrResults.SelectMany(x => x).OrderBy(x => x.CharactersList.Min(x => x.Rect.Left)).FirstOrDefault();
        }

        if (primaryBlock == null) return null;

        // 最も相応しい文字列ブロックと重なるすべての文字列ブロックのうち、文字数が最大のものを選択
        var minX = primaryBlock.CharactersList.Min(c => c.Rect.Left);
        var minY = primaryBlock.CharactersList.Min(c => c.Rect.Top);
        var maxRight = primaryBlock.CharactersList.Max(c => c.Rect.Right);
        var maxBot = primaryBlock.CharactersList.Max(c => c.Rect.Bottom);
        var rectOfPrimaryBlock =
            new SixLabors.ImageSharp.Rectangle(
                minX,
                minY,
                maxRight - minX,   // ← width は right–left
                maxBot - minY    // ← height は bottom–top
            );

        var intersectBlocks = allOcrResults
            .SelectMany(x => x)
            .Where(block =>
            {
                var bMinX = block.CharactersList.Min(c => c.Rect.Left);
                var bMinY = block.CharactersList.Min(c => c.Rect.Top);
                var bMaxR = block.CharactersList.Max(c => c.Rect.Right);
                var bMaxB = block.CharactersList.Max(c => c.Rect.Bottom);
                var rectOfBlock =
                    new SixLabors.ImageSharp.Rectangle(
                        bMinX,
                        bMinY,
                        bMaxR - bMinX,
                        bMaxB - bMinY
                    );
                return rectOfBlock.IntersectsWith(rectOfPrimaryBlock);
            });

        if (intersectBlocks == null) return null;

        PnOcrResultWordSet ret = new PnOcrResultWordSet(intersectBlocks, rightSide);

        return ret;
    }


    /// <summary>
    /// targetImage の中で targetRect で指定された小領域をトリミングし、
    /// 1～4 桁のページ番号 (連続した数字) の OCR を行い、各文字ごとの矩形＋可能性を返す。
    /// </summary>
    static List<PnOcrResultWord> PerformPageNumberOcrInternal(
        PnOcrEngineType ocrEngineType,
        int ocrEngineInstanceIndex,
        Image<L8> targetImage,
        SixLabors.ImageSharp.Rectangle targetRect,
        bool rightSide)
    {
        // ① targetRect で指定された領域を高速に切り出し
        using var region = targetImage.Clone(ctx => ctx.Crop(targetRect));

        //targetImage.Mutate(ctx => ctx.Invert());

        //using var mat = SuperImgUtil.ImageSharpL8ToMat(region);                         // ImageSharp→Mat 変換

        MemoryStream pngMs = new();
        region.SaveAsPng(pngMs);

        // ④ Mat → Pix（PNG 経由）
        //Cv2.ImEncode(".png", mat, out var imgPngBytes);
        using var pix = Pix.LoadFromMemory(pngMs.ToArray());

        Tesseract.Page page;

        var engine = GetOcrEngine(ocrEngineType, ocrEngineInstanceIndex);

        // OCR 実行
        lock (engine)
        {
            // 念のためロック (ocrEngineInstanceIndex でユニークになっているはずだが、バグがありえるため)
            page = engine.Process(pix);

            using (page)
            using (var iter = page.GetIterator())
            {
                var allChars = new List<PnOcrResultCharacter>();

                // ⑤ 文字（シンボル）単位で反復
                iter.Begin();
                do
                {
                    // 認識文字の取得
                    var txt = iter.GetText(PageIteratorLevel.Symbol);
                    if (string.IsNullOrEmpty(txt) || txt.Length != 1)
                        continue;

                    txt = Str.ZenkakuToHankaku(txt);
                    if (txt._IsEmpty()) continue;

                    char bestChar = txt[0];
                    if (!char.IsDigit(bestChar))
                        continue;

                    // バウンディングボックス取得
                    if (!iter.TryGetBoundingBox(PageIteratorLevel.Symbol, out var bBox))
                        continue;

                    // X1,Y1,X2,Y2 から (x, y, width, height) を計算
                    var charRect = new SixLabors.ImageSharp.Rectangle(
                        bBox.X1,                   // 左上 X
                        bBox.Y1,                   // 左上 Y
                        bBox.X2 - bBox.X1,         // 幅
                        bBox.Y2 - bBox.Y1          // 高さ
                    );

                    // 可能性リスト作成 (メイン + 代替／ChoiceIterator)
                    var list = new List<PnOcrCharacterPossibilitiesItem>();
                    int conf0 = (int)Math.Round(iter.GetConfidence(PageIteratorLevel.Symbol));
                    //if (conf0 > 20)
                    list.Add(new PnOcrCharacterPossibilitiesItem(bestChar, conf0));

                    var choiceIter = iter.GetChoiceIterator();
                    while (choiceIter.Next())
                    {
                        var alt = choiceIter.GetText();
                        if (string.IsNullOrEmpty(alt) || alt.Length != 1)
                            continue;
                        char c = alt[0];
                        if (!char.IsDigit(c) || c == bestChar)
                            continue;
                        int conf = (int)Math.Round(choiceIter.GetConfidence());
                        //if (conf > 20)
                        list.Add(new PnOcrCharacterPossibilitiesItem(c, conf));
                    }
                    // 降順ソート
                    list.Sort((a, b) => b.Possibility.CompareTo(a.Possibility));

                    if (list.Count > 0)
                    {
                        allChars.Add(new PnOcrResultCharacter(charRect, list));
                    }
                }
                while (iter.Next(PageIteratorLevel.Symbol));

                //// -------- 外れ値フィルタ --------
                //if (allChars.Count == 0) return new List<List<PnOcrResultCharacter>>();

                //var ws = allChars.Select(c => c.Rect.Width).OrderBy(w => w).ToArray();
                //float medianW2 = ws.Length % 2 == 1
                //                  ? ws[ws.Length / 2]
                //                  : (ws[ws.Length / 2 - 1] + ws[ws.Length / 2]) / 2f;

                //// ページ番号は幅が似通っている。中央値の 1.3 倍より大きい物は捨てる
                //allChars = allChars
                //            .Where(c => c.Rect.Width <= medianW2 * 1.3f)
                //            .ToList();

                if (allChars.Count == 0) return new List<PnOcrResultWord>();

                // ⑥ 左→右にソート
                allChars.Sort((a, b) => a.Rect.X.CompareTo(b.Rect.X));

                // ⑦ 文字間ギャップでクラスタリングし、一番大きい連続シーケンスを抽出
                var widths = allChars.Select(c => c.Rect.Width).OrderBy(w => w).ToList();
                float medianW = widths[widths.Count / 2];
                float gapThreshold = medianW * 1.5f;

                var clusters = new List<List<PnOcrResultCharacter>>();
                var current = new List<PnOcrResultCharacter> { allChars[0] };
                for (int i = 1; i < allChars.Count; i++)
                {
                    var prev = allChars[i - 1];
                    var curr = allChars[i];
                    float gap = curr.Rect.X - (prev.Rect.X + prev.Rect.Width);
                    if (gap <= gapThreshold)
                    {
                        current.Add(curr);
                    }
                    else
                    {
                        clusters.Add(current);
                        current = new List<PnOcrResultCharacter> { curr };
                    }
                }

                clusters.Add(current);

                List<PnOcrResultWord> ret = new();

                foreach (var cluster in clusters)
                {
                    PnOcrResultWord cluster2;

                    if (rightSide == false)
                    {
                        cluster2 = new(cluster.Take(4).ToList());
                    }
                    else
                    {
                        cluster2 = new(cluster.TakeLast(4).ToList());
                    }

                    ret.Add(cluster2);
                }

                return ret;
            }
        }
    }


    /// <summary>
    /// 二値化済みページから単語（文字クラスター）矩形を抽出する。
    /// 行という概念は使わず、隣接クラスタリングだけで判定。
    /// </summary>
    public static List<SixLabors.ImageSharp.Rectangle> OcrGetWordBlocks(
        OpenCvSharp.Mat targetImage,
        SixLabors.ImageSharp.Rectangle ignoreRegion)
    {
        // ---------- 0. 入力チェック ----------
        if (targetImage == null || targetImage.Empty())
            throw new ArgumentException("targetImage が null または空です。");

        // ---------- 1. 前処理 ----------
        using OpenCvSharp.Mat work = targetImage.Clone();

        // 1-1) 黒文字 / 白背景になるように自動反転
        double whiteRatio =
            (double)OpenCvSharp.Cv2.CountNonZero(work) / (work.Rows * work.Cols);
        if (whiteRatio > 0.8) OpenCvSharp.Cv2.BitwiseNot(work, work);

        // 1-2) ignoreRegion を黒塗り
        if (!ignoreRegion.IsEmpty)
        {
            OpenCvSharp.Rect ig = new OpenCvSharp.Rect(
                Math.Max(0, ignoreRegion.X),
                Math.Max(0, ignoreRegion.Y),
                Math.Max(0, Math.Min(ignoreRegion.Right, work.Cols) - ignoreRegion.X),
                Math.Max(0, Math.Min(ignoreRegion.Bottom, work.Rows) - ignoreRegion.Y));
            if (ig.Width > 0 && ig.Height > 0)
                OpenCvSharp.Cv2.Rectangle(work, ig, OpenCvSharp.Scalar.Black, -1);
        }

        // ---------- 2. 連結成分で 1 文字候補を列挙 ----------
        using OpenCvSharp.Mat labels = new OpenCvSharp.Mat();
        using OpenCvSharp.Mat stats = new OpenCvSharp.Mat();
        using OpenCvSharp.Mat centroids = new OpenCvSharp.Mat();
        int nLabels = OpenCvSharp.Cv2.ConnectedComponentsWithStats(
                          work, labels, stats, centroids,
                          OpenCvSharp.PixelConnectivity.Connectivity8,
                          OpenCvSharp.MatType.CV_32S);

        var charRects = new List<OpenCvSharp.Rect>();
        var charHeights = new List<int>();
        var charWidths = new List<int>();

        for (int id = 1; id < nLabels; id++)               // id=0 は背景
        {
            int w = stats.Get<int>(id, (int)OpenCvSharp.ConnectedComponentsTypes.Width);
            int h = stats.Get<int>(id, (int)OpenCvSharp.ConnectedComponentsTypes.Height);
            if (w < 3 || h < 30) continue;                 // 文字として小さ過ぎる

            int l = stats.Get<int>(id, (int)OpenCvSharp.ConnectedComponentsTypes.Left);
            int t = stats.Get<int>(id, (int)OpenCvSharp.ConnectedComponentsTypes.Top);

            charRects.Add(new OpenCvSharp.Rect(l, t, w, h));
            charWidths.Add(w);
            charHeights.Add(h);
        }
        if (charRects.Count == 0) return new();

        // 文書全体の中央値（横罫線除去にのみ利用）
        int docMedH = Median(charHeights);
        int docMedW = Median(charWidths);

        // ---------- 2-B. 横罫線だけは文書全体統計で除去 ----------
        bool IsHorizontalLine(OpenCvSharp.Rect rc) =>
                rc.Height <= docMedH * 0.7 &&
                rc.Width >= docMedW * 4;
        charRects = charRects.Where(r => !IsHorizontalLine(r)).ToList();
        if (charRects.Count == 0) return new();

        // ---------- 3. 文字近接クラスタリング ----------
        int xPad = Math.Max(3, (int)(docMedW * 0.6));     // 横パディング 60 %
        int yPad = Math.Max(1, (int)(docMedH * 0.2));     // 縦パディング 20 %

        int n = charRects.Count;
        var inflated = new OpenCvSharp.Rect[n];
        for (int i = 0; i < n; i++)
        {
            OpenCvSharp.Rect r = charRects[i];
            inflated[i] = new OpenCvSharp.Rect(
                Math.Max(0, r.X - xPad),
                Math.Max(0, r.Y - yPad),
                r.Width + xPad * 2,
                r.Height + yPad * 2);
        }

        // Union-Find で重なりグループ
        int[] parent = Enumerable.Range(0, n).ToArray();
        int Find(int i) { while (parent[i] != i) i = parent[i] = parent[parent[i]]; return i; }
        void Union(int a, int b)
        {
            int ra = Find(a), rb = Find(b);
            if (ra != rb) parent[rb] = ra;
        }
        for (int i = 0; i < n; i++)
            for (int j = i + 1; j < n; j++)
            {
                OpenCvSharp.Rect inter = inflated[i] & inflated[j];
                if (inter.Width > 0 && inter.Height > 0) Union(i, j);
            }

        var clusters = new Dictionary<int, List<OpenCvSharp.Rect>>();
        for (int i = 0; i < n; i++)
        {
            int root = Find(i);
            clusters.TryAdd(root, new List<OpenCvSharp.Rect>());
            clusters[root].Add(charRects[i]);
        }

        // ---------- 4. クラスタ内ローカル統計で“縦罫線”を除外 ----------
        var words = new List<SixLabors.ImageSharp.Rectangle>();

        foreach (List<OpenCvSharp.Rect> group in clusters.Values)
        {
            // 4-A) クラスタ内の統計
            int medH = Median(group.Select(r => r.Height).ToList());
            int medW = Median(group.Select(r => r.Width).ToList());

            // ---- 4-A'. スリム縦罫を判定して除去（重なり有無は問わない） -------------
            List<OpenCvSharp.Rect> filtered = new();

            double meanX = group.Average(r => r.X + r.Width / 2.0);   // クラスタ重心

            // X が左端側にある矩形を先にソートすると分かりやすい
            foreach (OpenCvSharp.Rect rc in group.OrderBy(r => r.X))
            {
                bool slim = rc.Width <= Math.Max(5, medW * 0.7);
                bool asp6 = (double)rc.Height / rc.Width >= 6.0;
                bool leftOfCenter = rc.Right <= meanX - medW * 0.3;

                // 「細長い」かつ「クラスタの左寄り」に位置する矩形は罫線とみなす
                if (slim && asp6 && leftOfCenter)
                    continue;                           // ←――― 捨てる

                filtered.Add(rc);                       // 残す
            }
            if (filtered.Count == 0) continue;          // 罫線だけのクラスタは破棄


            // ↓ 以降は filtered を使う --------------------------------------------
            medH = Median(filtered.Select(r => r.Height).ToList());
            medW = Median(filtered.Select(r => r.Width).ToList());

            bool IsVerticalLine(OpenCvSharp.Rect rc)
            {
                bool narrow = rc.Width <= Math.Max(5, medW * 0.7);
                bool taller = rc.Height >= medH * 1.1;
                bool aspLong = (double)rc.Height / rc.Width >= 6.0;
                return narrow && taller && aspLong;
            }
            var pureChars = filtered.Where(rc => !IsVerticalLine(rc)).ToList();
            if (pureChars.Count == 0) continue;
            /* ----------------------------------------------------- */


            // 4-C) 矩形合成（安全マージン 10 %）
            OpenCvSharp.Rect union = UnionRectList(pureChars, medH);
            words.Add(new SixLabors.ImageSharp.Rectangle(
                          union.X, union.Y, union.Width, union.Height));
        }

        // ---------- 5. 読み順ソート ----------
        return words
            .Where(x => x.Left >= 0 && x.Top >= 0 && x.Right <= targetImage.Width && x.Bottom <= targetImage.Height)
                .OrderBy(r => r.Y)
                .ThenBy(r => r.X)
                .ToList();
    }

    // =================== ヘルパメソッド群 ===================

    private static int Median(List<int> v)
    { v.Sort(); return v[v.Count / 2]; }


    private static OpenCvSharp.Rect UnionRectList(
        List<OpenCvSharp.Rect> rects, int charH)
    {
        int l = rects.Min(r => r.Left);
        int t = rects.Min(r => r.Top);
        int r = rects.Max(r => r.Right);
        int b = rects.Max(r => r.Bottom);

        int m = Math.Max(2, charH / 10);              // マージン 10 %
        l = Math.Max(0, l - m); t = Math.Max(0, t - m);
        r += m; b += m;

        return new OpenCvSharp.Rect(l, t, r - l, b - t);
    }









    /// <summary>
    /// 与えられた二値画像が縦書き (top-to-bottom) である確率を返す。
    /// 0.5 以下なら横書きの可能性が高い。
    /// </summary>
    /// <param name="image">CV_8UC1 二値画像 (4960 × 7016 px 程度を想定)</param>
    /// <returns>縦書き確率 (0.0–1.0)</returns>
    /// <exception cref="ArgumentNullException">image が null</exception>
    /// <exception cref="ArgumentException">image が CV_8UC1 以外</exception>
    public static double IsPaperVerticalWriting_GetProbability(Mat image)
    {
        // ── 0. 引数検証 ──────────────────────────────────────────────
        if (image == null || image.Empty())
            throw new ArgumentNullException(nameof(image), "image が null または空です。");

        if (image.Type() != MatType.CV_8UC1)
            throw new ArgumentException(
                $"image は {MatType.CV_8UC1} 形式でなければなりません。", nameof(image));

        using var imgCopy = image.Clone();

        // ── 1. 横方向スキャン (行→空行→行… を期待) ───────────────────
        double horizontalScore = ComputeLinearScore(imgCopy);

        // ── 2. 縦方向スキャン (画像を 90° 回転して同じ手法を適用) ───────
        using Mat rotated = new Mat();
        Cv2.Rotate(imgCopy, rotated, RotateFlags.Rotate90Clockwise);
        double verticalScore = ComputeLinearScore(rotated);

        // ── 3. 2 スコアを正規化し「縦書き確率」を得る ─────────────────
        double sum = horizontalScore + verticalScore + 1e-9; // 0 除算防止
        double verticalProbability = verticalScore / sum;

        // 念のため [0,1] にクランプ
        if (verticalProbability < 0.0) verticalProbability = 0.0;
        if (verticalProbability > 1.0) verticalProbability = 1.0;

        return verticalProbability;
    }

    // =========================================================================
    // 画像を横一列ずつ走査し「行らしさ」「行間広さ」「変動係数」を複合評価する
    // =========================================================================
    private static double ComputeLinearScore(Mat img)
    {
        int width = img.Cols;
        int height = img.Rows;

        // 段組みの影響を抑えるため横幅を 4 ブロックに分割
        int blockWidth = width / 4;
        double[] blockScores = new double[4];

        for (int blk = 0; blk < 4; blk++)
        {
            int startX = blk * blockWidth;
            int endX = (blk == 3) ? width : startX + blockWidth;

            // ── 1. 交差数系列を取得 ────────────────────────────
            var intersectionsPerRow = new int[height];
            long zeroLines = 0;         // 交差ゼロ行 (完全余白)
            double mean = 0.0;       // Welford 平均
            double m2 = 0.0;       // Welford 分散用
            long count = 0;

            for (int y = 0; y < height; y++)
            {
                int intersects = 0;
                bool inBlack = false;

                // 1 行の黒画素塊数をカウント
                for (int x = startX; x < endX; x++)
                {
                    if (img.At<byte>(y, x) == 0) // 黒
                    {
                        if (!inBlack)
                        {
                            intersects++;
                            inBlack = true;
                        }
                    }
                    else
                    {
                        inBlack = false;
                    }
                }

                intersectionsPerRow[y] = intersects;

                if (intersects == 0) zeroLines++;

                // Welford 法で平均と分散を更新
                count++;
                double delta = intersects - mean;
                mean += delta / count;
                double delta2 = intersects - mean;
                m2 += delta * delta2;
            }

            if (count == 0)
            {
                blockScores[blk] = 0.0;
                continue;
            }

            double variance = m2 / count;
            double stddev = Math.Sqrt(variance);
            double variationCoefficient = (mean > 0.0) ? stddev / mean : 0.0;
            double zeroRatio = (double)zeroLines / count;

            // ── 2. 行厚 / 行間を抽出して「広さ比率」を求める ─────────
            double threshold = Math.Max(1.0, mean); // mean が小さい場合は 1
            var lineThicknesses = new List<int>();
            var gapHeights = new List<int>();

            bool inLine = false;
            int runLen = 0;
            for (int y = 0; y < height; y++)
            {
                bool isLine = intersectionsPerRow[y] >= threshold;

                if (isLine == inLine)
                {
                    runLen++;
                }
                else
                {
                    // 切替え時に前 run を記録
                    if (runLen > 0)
                    {
                        if (inLine)
                            lineThicknesses.Add(runLen);
                        else
                            gapHeights.Add(runLen);
                    }
                    inLine = isLine;
                    runLen = 1;
                }
            }
            // 最終 run
            if (runLen > 0)
            {
                if (inLine)
                    lineThicknesses.Add(runLen);
                else
                    gapHeights.Add(runLen);
            }

            double separationRatio = 0.0; // 行間の広さ指標 (0=詰まっている, 1=広い)
            if (lineThicknesses.Count > 0 && gapHeights.Count > 0)
            {
                // 中央値を使用し外れ値の影響を抑える
                int medianLine = Median2(lineThicknesses);
                int medianGap = Median2(gapHeights);

                separationRatio = (double)medianGap /
                                  (medianLine + medianGap + 1e-9);
            }

            // ── 3. 3 つの指標を合成してブロックスコアを算出 ────────
            //      weight = {variation:0.4, zeroLine:0.2, separation:0.4}
            double score =
                (variationCoefficient * 0.4) +
                (zeroRatio * 0.2) +
                (separationRatio * 0.4);

            if (score < 0.0) score = 0.0;
            if (score > 1.0) score = 1.0;

            blockScores[blk] = score;
        }

        // ブロック平均をページの横方向スコアとして返す
        return blockScores.Average();
    }

    // ──────────────────────────────────────────────────────────────
    // 便利メソッド：中央値 (旧 Median → Median2)
    // ──────────────────────────────────────────────────────────────
    private static int Median2(List<int> data)
    {
        if (data == null || data.Count == 0) return 0;

        data.Sort();
        int mid = data.Count / 2;
        return (data.Count % 2 == 1)
            ? data[mid]
            : (data[mid - 1] + data[mid]) / 2;
    }

}



#endregion













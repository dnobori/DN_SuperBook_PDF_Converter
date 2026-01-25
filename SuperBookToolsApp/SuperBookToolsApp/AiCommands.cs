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

using SuperBookTools;
using SuperBookTools.App;

namespace SuperBookTools.App
{
    public static class SuperBookExternalTools
    {
        public static readonly ImageMagickUtil ImageMagick = new ImageMagickUtil(new ImageMagickOptions(
            Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\ImageMagick-portable-Q16-HDRI-x64\magick.exe"),
            Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\ImageMagick-portable-Q16-HDRI-x64\mogrify.exe"),
            Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\exiftool-13.30_64\exiftool.exe"),
            Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\QPDF\bin\qpdf.exe"),
            Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\pdfcpu\pdfcpu.exe")
        ));

        public static readonly FfMpegUtil FfMpeg = new FfMpegUtil(new FfMpegUtilOptions(
            Path.Combine(Env.AppRootDir, @"_dummy.exe"),
            Path.Combine(Env.AppRootDir, @"_dummy.exe")));

        public static readonly PdfYomitokuLib YomiToku = new PdfYomitokuLib(Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\yomitoku"));

        public static readonly AiUtilBasicSettings Settings = new AiUtilBasicSettings
        {
            AiTest_RealEsrgan_BaseDir = Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\RealEsrgan\RealEsrgan_Repo"),
            AiTest_TesseractOCR_Data_Dir = Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\TesseractOCR_Data"),
        };
        public static readonly AiTask Task = new AiTask(Settings, FfMpeg);

        public const string Post_OCR_Dir = "Post_OCR_Dir";
    }

    public static partial class Commands
    {
        [ConsoleCommand(
            "ConvertPdf command",
            "ConvertPdf [srcDir] [/dst:dstDir] [/ocr:yes|no] [/skip:yes|no] [/margin:N] [/max-pages:N] [/debug:yes|no]",
            "Convert PDF files with image enhancement and OCR.\n" +
            "Options:\n" +
            "  /dst:path       - Destination directory (required)\n" +
            "  /ocr:yes|no     - Perform Japanese AI-OCR using YomiToku (default: no)\n" +
            "  /skip:yes|no    - Skip RealEsrgan image enhancement (default: no)\n" +
            "  /margin:N       - Margin percentage for trimming (default: 7)\n" +
            "  /max-pages:N    - Maximum pages to process, for testing (default: unlimited)\n" +
            "  /debug:yes|no   - Save debug PNG files (default: no)")]
        public static async Task<int> ConvertPdf(ConsoleService c, string cmdName, string str)
        {
            ConsoleParam[] args =
            {
                new ConsoleParam("[srcDir]", ConsoleService.Prompt, "Source directory path: ", ConsoleService.EvalNotEmpty, null),
                new ConsoleParam("dst", ConsoleService.Prompt, "Destination directory path: ", ConsoleService.EvalNotEmpty, null),
                new ConsoleParam("ocr", ConsoleService.Prompt, "Perform Japanese High-Quality OCR? (Y/N): ", null, null),
                new ConsoleParam("skip", null, null, null, null),
                new ConsoleParam("margin", null, null, null, null),
                new ConsoleParam("max-pages", null, null, null, null),
                new ConsoleParam("debug", null, null, null, null),
            };
            ConsoleParamValueList vl = c.ParseCommandList(cmdName, str, args);

            string srcDir = vl.DefaultParam.StrValue;
            string dstDir = vl["dst"].StrValue;

            srcDir = PP.RemoveLastSeparatorChar(await Lfs.NormalizePathAsync(srcDir, normalizeRelativePathIfSupported: true));
            dstDir = PP.RemoveLastSeparatorChar(await Lfs.NormalizePathAsync(dstDir, normalizeRelativePathIfSupported: true));

            $"- Source Dir: \"{srcDir}\""._Print();
            $"- Destination Dir: \"{dstDir}\""._Print();

            if (srcDir._IsSamei(dstDir))
            {
                throw new CoresException("srcDir must not be same to dstDir.");
            }

            await Lfs.CreateDirectoryAsync(dstDir);

            bool skipRealesrgan = vl["skip"].BoolValue;
            int marginPercent = vl["margin"].IntValue > 0 ? vl["margin"].IntValue : 7;
            int maxPages = vl["max-pages"].IntValue > 0 ? vl["max-pages"].IntValue : int.MaxValue;
            bool saveDebugPng = vl["debug"].BoolValue;

            SuperPerformPdfOptions options = new SuperPerformPdfOptions
            {
                SkipRealesrgan = skipRealesrgan,
                MarginPercent = marginPercent,
                MaxPagesForDebug = maxPages,
                SaveDebugPng = saveDebugPng,
            };

            bool performOcr = vl["ocr"].BoolValue;

            // Print active options
            $"- Options: margin={marginPercent}%, max-pages={( maxPages == int.MaxValue ? "unlimited" : maxPages.ToString() )}, skip={skipRealesrgan}, ocr={performOcr}, debug={saveDebugPng}"._Print();

            if (skipRealesrgan)
            {
                ""._Print();
                "***"._Print();
                $"The \"skip\" option is enabled. RealEsrgan image enhancement will be skipped."._Print();
                "***"._Print();
                ""._Print();
            }

            if (performOcr)
            {
                ""._Print();
                "***"._Print();
                $"The \"ocr\" option is enabled. This OCR feature uses \"YomiKaku\" AI engine published by kotaro.kinoshita-san. Plesae read the https://github.com/kotaro-kinoshita/yomitoku/blob/cba0a134e0d2ad3bfdce163231b3cb91de07928e/README.md license document."._Print();
                "***"._Print();
                ""._Print();
            }

            var srcFiles = (await Lfs.EnumDirectoryAsync(srcDir, true)).Where(x => x.IsFile && x.Name.StartsWith("_") == false && x.Name._IsExtensionMatch(".pdf")).OrderBy(x => x.FullPath, StrCmpi)._Shuffle().ToList();

            int numTotal = srcFiles.Count();
            int numOk = 0;
            int numError = 0;
            int numSkip = 0;

            $"Total {numTotal} Files"._Error();

            int currentNumber = 0;

            List<string> errorFilesList = new();

            foreach (var src in srcFiles)
            {
                currentNumber++;
                string relativePath = PP.GetRelativeFileName(src.FullPath, srcDir);
                string dstPath = PP.Combine(dstDir, relativePath);

                $"<< {currentNumber} / {numTotal} >> '{src.FullPath}' Start"._Error();

                try
                {
                    if (await SuperPdfUtil.PerformPdfAsync(src.FullPath, dstPath, options) == false)
                    {
                        numSkip++;
                        $"<< {currentNumber} / {numTotal} >> '{src.FullPath}' Skip"._Error();
                    }
                    else
                    {
                        numOk++;
                        $"<< {currentNumber} / {numTotal} >> '{src.FullPath}' OK"._Error();
                    }
                }
                catch (Exception ex)
                {
                    Con.WriteLine($"<< {currentNumber} / {numTotal} >> Error: {src.FullPath} -> {dstPath}");
                    ex._Error();
                    errorFilesList.Add(src.FullPath);
                    numError++;
                }
            }

            if (performOcr)
            {
                Con.WriteLine("Performing Japanese OCR started ...");

                await SuperBookExternalTools.YomiToku.PerformOcrDirAsync(dstDir, PP.Combine(dstDir, SuperBookExternalTools.Post_OCR_Dir), SuperBookExternalTools.Post_OCR_Dir);

                Con.WriteLine("Performing Japanese OCR completed.");
            }

            if (errorFilesList.Count >= 1)
            {
                $"--- Error files ---"._Error();
                foreach (var errFile in errorFilesList)
                {
                    $"- {errFile}"._Error();
                }
            }

            $"\n\n<< ConvertPdf Result >>\nnumTotal = {numTotal}, numSkip = {numSkip}, numOk = {numOk}, numError = {numError}\n\n"._Error();

            return 0;
        }
    }
}

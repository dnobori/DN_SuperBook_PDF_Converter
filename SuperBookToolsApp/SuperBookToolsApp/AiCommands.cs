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

        public static readonly AiUtilBasicSettings Settings = new AiUtilBasicSettings
        {
            AiTest_RealEsrgan_BaseDir = Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\RealEsrgan\RealEsrgan_Repo"),
            AiTest_TesseractOCR_Data_Dir = Path.Combine(Env.AppRootDir, @"..\external_tools\external_tools\image_tools\TesseractOCR_Data"),
        };
        public static readonly AiTask Task = new AiTask(Settings, FfMpeg);
    }

    public static partial class Commands
    {
        [ConsoleCommand(
            "ConvertPdf command",
            "ConvertPdf [srcDir] [/dst:dstDir]",
            "ConvertPdf command")]
        public static async Task<int> ConvertPdf(ConsoleService c, string cmdName, string str)
        {
            ConsoleParam[] args =
            {
                new ConsoleParam("[srcDir]", ConsoleService.Prompt, "Source directory path: ", ConsoleService.EvalNotEmpty, null),
                new ConsoleParam("dst", ConsoleService.Prompt, "Destination directory path: ", ConsoleService.EvalNotEmpty, null),
            };
            ConsoleParamValueList vl = c.ParseCommandList(cmdName, str, args);

            string srcDir = (vl.DefaultParam.StrValue ?? throw new CoresException("srcDir is empty")).Trim().Trim('"');
            string dstDir = (vl["dst"].StrValue ?? throw new CoresException("dstDir is empty")).Trim().Trim('"');
            srcDir = Path.GetFullPath(srcDir);
            srcDir = Path.GetFullPath(srcDir);

            if (srcDir._IsSamei(dstDir) == true)
            {
                throw new CoresException("srcDir must not be same to dstDir.");
            }

            SuperPerformPdfOptions options = new SuperPerformPdfOptions {/* MaxPagesForDebug = 120, SaveDebugPng = true, SkipRealesrgan = true */ };

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

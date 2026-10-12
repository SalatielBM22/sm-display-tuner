# SM Display Tuner

Ajuste de cor **ao vivo** para monitores no Windows, no estilo do que o BetterDisplay faz no macOS:
sliders que mexem direto na curva de cor da GPU enquanto você olha a tela, padrões de teste sem
gerenciamento de cor, medição visual do gama de cada canal e exportação do resultado como perfil ICC
que o Windows carrega sozinho no login.

Feito para painéis de notebook ruins (TN, 6 bits, ~50 % sRGB), onde perfis "corretos" pioram a
imagem: o gerenciamento de cor comprime tudo para o gamut minúsculo do painel e corta os tons
saturados. Aqui o perfil declara o painel como sRGB (o mesmo truque do "Generic RGB" no Mac) e o
ajuste real vive na curva da GPU.

> Projeto da **SM Acoustics** · Rust · licença MIT

## Recursos
- Brilho, gama alvo, contraste (curva S), preto e temperatura de cor (calculada pelas primárias do EDID)
- Ganho, gama e preto por canal R/G/B
- Padrões de teste: gradientes de 256 níveis e 32 degraus, quase-preto/quase-branco por canal,
  ColorChecker, rampas de verde e de pele
- **Medir gama**: casa um quadrado sólido com um padrão de linhas de 1 px (25 % e 50 %) e calcula o
  gama real de cada canal; a correção passa a partir da medição
- Presets em JSON; último ajuste salvo automaticamente
- Exporta ICC v2.4 (primárias sRGB + curva sRGB + `vcgt` com a sua curva), instala e associa ao monitor
  (UAC só na primeira vez), volta ao perfil anterior com um clique
- Ao fechar sem ativar, restaura a curva original da GPU

## Requisitos
- Windows 10/11, qualquer GPU que aceite `SetDeviceGammaRamp`
- **ACM ("Gerenciar cor automaticamente") desligado** — com ACM/HDR o Windows ignora a curva legada
- Curvas muito agressivas exigem `GdiIcmGammaRange = 256` (o app oferece liberar, com UAC)

## Compilar
```powershell
cargo build --release
```
Com o toolchain `x86_64-pc-windows-gnu` (sem Visual Studio) o `eframe` precisa de `dlltool.exe` e
`as.exe` no `PATH` (binutils do MinGW-w64, ex.: WinLibs). Com o toolchain MSVC basta o Build Tools.

Autoteste sem interface (lê/escreve/restaura a curva e grava um ICC de teste):
```powershell
sm-display-tuner.exe --selftest   # resultado em Documentos\SM_ICC_AUO2E3C\tuner\selftest.txt
```

## Estrutura
| Arquivo | Conteúdo |
|---|---|
| `src/win.rs` | gamma ramp, alvo de exibição (QueryDisplayConfig), associação de perfis (mscms), elevação |
| `src/model.rs` | parâmetros → curva 3×256, colorimetria (matrizes, Bradford, CCT) |
| `src/icc.rs` | escritor de ICC v2.4 com `vcgt` |
| `src/patterns.rs` | padrões de teste e medição de gama |
| `src/main.rs` | interface (egui) e modos sem interface |

## Roadmap
- Modo de compatibilidade com a escala de sliders do BetterDisplay e importação do `.plist`
- Saturação/matriz de cor por todo o sistema
- Perfis por aplicativo (troca automática ao abrir um jogo/programa)
- Suporte a múltiplos monitores

## Licença
MIT — veja [LICENSE](LICENSE).

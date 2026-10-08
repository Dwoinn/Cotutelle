# Paquet RPM de l'agent, construit par scripts/build-rpm.sh à partir d'un
# binaire déjà compilé : pkg_version, srcdir et agent_bin viennent du script.

# Le binaire est déjà optimisé et épuré par cargo.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none
# gzip : lisible par les rpm anciens (openSUSE Leap, RHEL 8).
%global _binary_payload w9.gzdio

Name:           cotutelle-agent
Version:        %{pkg_version}
Release:        1
Summary:        Agent de contrôle parental Cotutelle
License:        AGPL-3.0-or-later
URL:            https://github.com/Dwoinn/cotutelle
Requires:       systemd
Requires:       nftables
Recommends:     systemd-resolved
Recommends:     /usr/bin/notify-send
Recommends:     xdg-utils

%description
Applique sur cet ordinateur les règles définies par les parents sur le
serveur Cotutelle : filtrage DNS, horaires, temps d'écran et verrouillage
de session. Continue de protéger sans connexion au serveur.

%install
install -D -m 0755 %{agent_bin} %{buildroot}%{_bindir}/cotutelle-agent
install -D -m 0644 %{srcdir}/packaging/cotutelle-agent.service %{buildroot}/usr/lib/systemd/system/cotutelle-agent.service
install -D -m 0644 %{srcdir}/packaging/cotutelle.desktop %{buildroot}%{_datadir}/applications/cotutelle.desktop
install -D -m 0644 %{srcdir}/web/src/lib/assets/favicon.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/cotutelle.svg
install -D -m 0644 %{srcdir}/README.md %{buildroot}%{_datadir}/doc/cotutelle-agent/README.md
install -D -m 0644 %{srcdir}/NOTICE.md %{buildroot}%{_datadir}/doc/cotutelle-agent/NOTICE.md
install -D -m 0644 %{srcdir}/LICENSE %{buildroot}%{_datadir}/doc/cotutelle-agent/LICENSE

%files
%{_bindir}/cotutelle-agent
/usr/lib/systemd/system/cotutelle-agent.service
%{_datadir}/applications/cotutelle.desktop
%{_datadir}/icons/hicolor/scalable/apps/cotutelle.svg
%{_datadir}/doc/cotutelle-agent

%post
install -d -m 0700 /var/lib/cotutelle
systemctl daemon-reload || :
if [ -f /var/lib/cotutelle/agent.json ]; then
    # Mise à jour d'un appareil déjà rattaché : on relance l'agent.
    systemctl enable cotutelle-agent.service >/dev/null 2>&1 || :
    systemctl restart cotutelle-agent.service || :
else
    cat <<'MSG'

Cotutelle est installé. Pour rattacher cet ordinateur à votre serveur :

  1. Dans l'interface parents : Appareils > Ajouter un ordinateur.
  2. Ici, avec le code affiché :
       sudo cotutelle-agent enroll --server http://ADRESSE:8080 --code XXXX-XXXX
       sudo systemctl enable --now cotutelle-agent

MSG
fi

%preun
# $1 vaut 0 à la désinstallation, 1 lors d'une mise à jour.
if [ "$1" -eq 0 ]; then
    systemctl disable --now cotutelle-agent.service >/dev/null 2>&1 || :
    # Retire les règles DNS et nftables posées par l'agent.
    %{_bindir}/cotutelle-agent release >/dev/null 2>&1 || :
fi

%postun
systemctl daemon-reload >/dev/null 2>&1 || :

# Test SMB server for the spike: SMB2/3 only, one read-only share "media", user coyotl.
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y --no-install-recommends samba && rm -rf /var/lib/apt/lists/*
RUN useradd -M -s /usr/sbin/nologin coyotl && (echo spike; echo spike) | smbpasswd -s -a coyotl
RUN printf '[global]\n  server min protocol = SMB2_10\n  server role = standalone server\n  map to guest = never\n  log level = 1\n  disable netbios = yes\n[media]\n  path = /media\n  read only = yes\n  valid users = coyotl\n' > /etc/samba/smb.conf
EXPOSE 445
CMD ["smbd", "--foreground", "--no-process-group", "--debug-stdout"]
